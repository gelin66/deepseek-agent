#!/usr/bin/env python3
"""Preregistered DeepSeek code-revision A/B for M7 Agent convergence.

The evaluator talks only to the canonical ``app-server --stdio`` Run API.
Committed output is limited to hashes and aggregate summaries; raw results are
written mode 0600 under the Git-ignored ``eval/results`` directory.
"""

from __future__ import annotations

import argparse
import copy
import hashlib
import importlib.util
import json
import os
import shutil
import sqlite3
import stat
import statistics
import subprocess
import sys
import tempfile
import time
import unittest
import uuid
from collections import Counter, defaultdict
from fractions import Fraction
from pathlib import Path
from typing import Any
from unittest import mock


ROOT = Path(__file__).resolve().parents[1]
MANIFEST_PATH = ROOT / "eval/manifests/m7-a-agent-convergence-ab-v1.json"
CANARY_PATH = ROOT / "scripts/eval-m6-writer-canary.py"
RESULT_SCHEMA = "codewhale.eval.m7-agent-convergence.v2"
RUN_API_SCHEMA = 9
MODEL = "deepseek-v4-flash"
TASK_IDS = ("t1", "t2", "t3", "t4", "t5")
VARIANTS = ("baseline", "candidate")
USAGE_FIELDS = (
    "input_tokens",
    "output_tokens",
    "cache_hit_tokens",
    "cache_miss_tokens",
    "cache_write_tokens",
    "reasoning_tokens",
    "reasoning_replay_tokens",
)
SECRET_FAILURES = {
    "key_in_argv",
    "key_in_fixture",
    "key_in_protocol",
    "key_in_result",
    "key_in_state",
    "key_in_stderr",
}
FROZEN_STATUS = "frozen_before_candidate_live_api_after_harness_hardening"


class EvaluationError(RuntimeError):
    def __init__(self, code: str, details: dict[str, Any] | None = None) -> None:
        super().__init__(code)
        self.code = code
        self.details = details or {}


def require(condition: bool, code: str, details: dict[str, Any] | None = None) -> None:
    if not condition:
        raise EvaluationError(code, details)


def load_module(path: Path, name: str) -> Any:
    spec = importlib.util.spec_from_file_location(name, path)
    require(spec is not None and spec.loader is not None, f"{name}_unavailable")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


CANARY = load_module(CANARY_PATH, "codewhale_m7_run_api_support")


def canonical_bytes(value: Any) -> bytes:
    return json.dumps(
        value, ensure_ascii=False, sort_keys=True, separators=(",", ":")
    ).encode("utf-8")


def sha256_bytes(value: bytes) -> str:
    return "sha256:" + hashlib.sha256(value).hexdigest()


def canonical_hash(value: Any) -> str:
    return sha256_bytes(canonical_bytes(value))


def file_hash(path: Path) -> str:
    return sha256_bytes(path.read_bytes())


def git_tree(revision: str) -> str:
    result = subprocess.run(
        ["git", "rev-parse", f"{revision}^{{tree}}"],
        cwd=ROOT,
        env=safe_env(),
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        timeout=10,
        check=False,
        text=True,
    )
    require(result.returncode == 0, "source_revision_unavailable")
    value = result.stdout.strip()
    require(len(value) == 40 and all(character in "0123456789abcdef" for character in value), "source_tree_invalid")
    return value


def remaining_timeout(deadline: float, maximum: float = 30.0) -> float:
    remaining = deadline - time.monotonic()
    require(remaining > 0, "arm_deadline_exceeded")
    return min(maximum, remaining)


def load_manifest() -> dict[str, Any]:
    try:
        value = json.loads(MANIFEST_PATH.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise EvaluationError("manifest_unavailable") from error
    require(
        value.get("schema") == "codewhale.eval.m7-agent-convergence-plan.v1"
        and tuple(value.get("tasks", {})) == TASK_IDS,
        "manifest_invalid",
    )
    return value


MANIFEST = load_manifest()
RESOURCES = MANIFEST["resources"]


def manifest_content_hash() -> str:
    value = copy.deepcopy(MANIFEST)
    value.pop("frozen_hashes", None)
    return canonical_hash(value)


def fixture_path(task_id: str) -> Path:
    return ROOT / MANIFEST["tasks"][task_id]["fixture"]


def snapshot_tree(root: Path) -> list[dict[str, Any]]:
    entries: list[dict[str, Any]] = []
    for path in sorted(root.rglob("*")):
        if not path.is_file() or ".git" in path.relative_to(root).parts:
            continue
        metadata = path.lstat()
        require(stat.S_ISREG(metadata.st_mode) and not path.is_symlink(), "fixture_shape_invalid")
        entries.append(
            {
                "path": path.relative_to(root).as_posix(),
                "mode": stat.S_IMODE(metadata.st_mode),
                "sha256": file_hash(path),
            }
        )
    return entries


def fixture_hash(task_id: str) -> str:
    return canonical_hash(snapshot_tree(fixture_path(task_id)))


def safe_env() -> dict[str, str]:
    return CANARY.safe_env()


def run_git(workspace: Path, *arguments: str, environment: dict[str, str] | None = None) -> str:
    result = subprocess.run(
        ["git", *arguments],
        cwd=workspace,
        env=environment or safe_env(),
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=30,
        check=False,
        text=True,
    )
    require(
        result.returncode == 0,
        "git_failed",
        {"arguments_sha256": canonical_hash(list(arguments)), "returncode": result.returncode},
    )
    return result.stdout.rstrip()


def materialize_fixture(task_id: str, destination: Path) -> str:
    source = fixture_path(task_id)
    require(fixture_hash(task_id) == MANIFEST["tasks"][task_id]["fixture_tree_sha256"], "fixture_hash_mismatch")
    shutil.copytree(source, destination)
    environment = safe_env()
    environment.update(
        GIT_AUTHOR_DATE="2026-07-22T00:00:00Z",
        GIT_COMMITTER_DATE="2026-07-22T00:00:00Z",
    )
    run_git(destination, "init", "-q", "-b", "main", environment=environment)
    run_git(destination, "add", "--", ".", environment=environment)
    run_git(
        destination,
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "user.name=CodeWhale Eval",
        "-c",
        "user.email=eval.invalid",
        "commit",
        "-q",
        "-m",
        f"M7-A frozen fixture {fixture_path(task_id).name}",
        environment=environment,
    )
    base = run_git(destination, "rev-parse", "HEAD", environment=environment)
    require(
        base == MANIFEST["tasks"][task_id]["fixture_base_commit"]
        and run_git(destination, "status", "--porcelain=v1", "--untracked-files=all", environment=environment) == ""
        and run_git(destination, "symbolic-ref", "-q", "HEAD", environment=environment) == "refs/heads/main",
        "fixture_git_identity_mismatch",
    )
    return base


def verifier_spec(task_id: str, *, resolved: bool) -> dict[str, Any]:
    acceptance = MANIFEST["tasks"][task_id]["acceptance_id"]
    name = f"{acceptance}-exact"
    command = {
        "name": name,
        "program": "/usr/bin/python3",
        "args": ["-I", "-B", "_eval_verifier.py", "."],
        "cwd": "",
    }
    step = {
        "id": name,
        "program": command["program"],
        "args": command["args"],
        "cwd": "",
        "env": {"PYTHONDONTWRITEBYTECODE": "1"} if resolved else {},
        "timeout_ms": 600_000,
    }
    return {
        "verifier_id": "run_verifiers",
        "parameters": {
            "profile": "exact",
            "level": "quick",
            "max_python_files": 200,
            "commands": [command],
        },
        "plan": {"steps": [step]},
    }


def task_definition(task_id: str) -> dict[str, Any]:
    frozen = MANIFEST["tasks"][task_id]
    return {
        "objective": frozen["objective"],
        "constraints": frozen["constraints"],
        "non_goals": frozen["non_goals"],
        "acceptance": [
            {
                "kind": "verifier",
                "id": frozen["acceptance_id"],
                "description": f"{frozen['name']} 的冻结确定性验收",
                "evidence_policy": frozen["evidence_policy"],
                "verifier": verifier_spec(task_id, resolved=False),
            }
        ],
    }


def runtime_task_definition(task_id: str, variant: str) -> dict[str, Any]:
    definition = task_definition(task_id)
    if variant == "candidate":
        definition["acceptance"][0]["verifier"] = verifier_spec(
            task_id,
            resolved=True,
        )
    return definition


def start_command(task_id: str, workspace: Path, request_id: str) -> dict[str, Any]:
    task = MANIFEST["tasks"][task_id]
    limits = {
        "max_turns": RESOURCES["max_logical_model_requests_per_arm"],
        "max_model_requests": RESOURCES["max_logical_model_requests_per_arm"],
        "max_model_retries": RESOURCES["max_runtime_retries_per_arm"],
        "max_tool_calls": RESOURCES["max_tool_calls_per_arm"],
        "max_depth": task["max_depth"],
        "max_concurrent_children": task["max_concurrent_children"],
        "model_event_idle_ms": 120_000,
        "wall_time_ms": RESOURCES["runtime_wall_time_seconds"] * 1000,
    }
    return {
        "schema_version": RUN_API_SCHEMA,
        "request_id": request_id,
        "command": {
            "kind": "start",
            "task": task_definition(task_id),
            "workspace": str(workspace.resolve()),
            "model": MANIFEST["experiment"]["model"],
            "reasoning_effort": RESOURCES["reasoning_effort"],
            "max_output_tokens": RESOURCES["max_output_tokens_per_request"],
            "max_api_requests": RESOURCES["max_physical_api_attempts_per_arm"],
            "streaming": RESOURCES["streaming"],
            "tool_policy": {
                "enabled": True,
                "allowed": MANIFEST["tool_policy"]["root_tools"],
                "denied": [],
            },
            "limits": limits,
            "controls": {
                "write_execution_mode": MANIFEST["tool_policy"]["write_execution_mode"],
                "auto_approve": RESOURCES["auto_approve"],
                "trust_mode": RESOURCES["trust_mode"],
                "allow_sandbox_elevation": RESOURCES["allow_sandbox_elevation"],
                "interactive": RESOURCES["interactive"],
                "sandbox": RESOURCES["sandbox"],
            },
        },
    }


def event_kind(stored: dict[str, Any]) -> str:
    event = stored.get("event", {})
    return event.get("kind", "") if isinstance(event, dict) else ""


def event_values(events: list[dict[str, Any]], kind: str) -> list[dict[str, Any]]:
    return [stored["event"] for stored in events if event_kind(stored) == kind]


def events(
    client: Any,
    run_id: str,
    request_id: str,
    expected_schema: int,
    deadline: float,
) -> list[dict[str, Any]]:
    result = client.call(
        CANARY.query("events", run_id, request_id),
        timeout_seconds=remaining_timeout(deadline),
    )
    value = result.get("events")
    require(
        result.get("kind") == "events"
        and result.get("run_id") == run_id
        and isinstance(value, list),
        "events_missing",
    )
    require(
        [event.get("sequence") for event in value] == list(range(1, len(value) + 1)),
        "event_sequence_invalid",
    )
    require(
        all(
            event.get("schema_version") == expected_schema
            and event.get("run_id") == run_id
            for event in value
        ),
        "event_schema_invalid",
    )
    return value


def run_created(events: list[dict[str, Any]]) -> dict[str, Any]:
    values = event_values(events, "run_created")
    return values[0].get("request", {}) if len(values) == 1 else {}


def terminal_state(run: dict[str, Any]) -> str | None:
    terminal = run.get("terminal")
    return terminal.get("state") if isinstance(terminal, dict) else None


def terminal_summary(run: dict[str, Any]) -> dict[str, Any]:
    terminal = run.get("terminal", {})
    reason = terminal.get("reason") if isinstance(terminal, dict) else None
    return {
        "state": terminal_state(run),
        "reason_code": " ".join(str(reason or "").split())[:256] or None,
        "reason_sha256": sha256_bytes(str(reason).encode()) if reason else None,
    }


def usage_summary(run: dict[str, Any]) -> dict[str, Any]:
    accounting = run.get("accounting", {})
    usage = run.get("usage", {})
    root = accounting.get("root", {})
    child = accounting.get("child", {})
    surface_usage = accounting.get("surface_usage", [])
    try:
        hard_request_limit = accounting.get("hard_request_limit")
        if hard_request_limit is not None:
            hard_request_limit = int(hard_request_limit)
        requests = {
            key: int(root[key]) + int(child[key])
            for key in ("started", "completed", "in_flight")
        }
        result = {
            "hard_request_limit": hard_request_limit,
            "requests": requests,
            "root": {key: int(root[key]) for key in ("started", "completed", "in_flight", "retries")},
            "child": {key: int(child[key]) for key in ("started", "completed", "in_flight", "retries")},
            "runtime_retries": int(run.get("runtime_retries", 0)),
            "transport_retries": int(accounting.get("transport_retries", 0)),
            "billing_unknown_attempts": int(accounting.get("billing_unknown_attempts", 0)),
            "usage_responses": int(accounting.get("usage_responses", 0)),
            "usage_missing_responses": int(accounting.get("usage_missing_responses", 0)),
            "incomplete_responses": int(accounting.get("incomplete_responses", 0)),
            "unpriced_usage_responses": int(accounting.get("unpriced_usage_responses", 0)),
            "records_after_seal": int(accounting.get("records_after_seal", 0)),
            "usage": {field: int(usage.get(field, 0)) for field in USAGE_FIELDS},
            "cost_nanousd": int(accounting.get("cost_nanousd", 0)),
            "cost_nanocny": int(accounting.get("cost_nanocny", 0)),
            "complete": accounting.get("complete"),
            "usage_complete": accounting.get("usage_complete"),
            "billing_unknown": accounting.get("billing_unknown"),
            "unpriced": accounting.get("unpriced"),
            "sealed": accounting.get("sealed"),
            "surface_usage": [
                {
                    "surface": bucket["surface"],
                    "model": bucket["model"],
                    "response_count": int(bucket["response_count"]),
                    "usage_response_count": int(bucket["usage_response_count"]),
                    "usage": {
                        field: int(bucket.get("usage", {}).get(field, 0))
                        for field in USAGE_FIELDS
                    },
                    "cost_nanousd": int(bucket["cost_nanousd"]),
                    "cost_nanocny": int(bucket["cost_nanocny"]),
                }
                for bucket in surface_usage
            ],
        }
    except (KeyError, TypeError, ValueError) as error:
        raise EvaluationError("accounting_shape_invalid") from error
    surface_usage_sum = {
        field: sum(bucket["usage"][field] for bucket in result["surface_usage"])
        for field in USAGE_FIELDS
    }
    result["surface_totals_valid"] = (
        sum(bucket["response_count"] for bucket in result["surface_usage"])
        == result["usage_responses"]
        and sum(bucket["usage_response_count"] for bucket in result["surface_usage"])
        == result["usage_responses"]
        and surface_usage_sum == result["usage"]
        and sum(bucket["cost_nanousd"] for bucket in result["surface_usage"])
        == result["cost_nanousd"]
        and sum(bucket["cost_nanocny"] for bucket in result["surface_usage"])
        == result["cost_nanocny"]
    )
    result["surface_identity_observed"] = bool(result["surface_usage"])
    result["surface_identity_mismatch"] = result["surface_identity_observed"] and any(
        bucket["surface"] != "standard_chat" or bucket["model"] != MODEL
        for bucket in result["surface_usage"]
    )
    result["surface_identity_valid"] = result["surface_identity_observed"] and all(
        bucket["surface"] == "standard_chat" and bucket["model"] == MODEL
        for bucket in result["surface_usage"]
    )
    result["budget_identity_valid"] = (
        result["hard_request_limit"]
        == RESOURCES["max_physical_api_attempts_per_arm"]
    )
    result["retry_attribution_valid"] = (
        result["root"]["retries"] + result["child"]["retries"]
        == result["transport_retries"]
    )
    result["retry_treatment_valid"] = (
        result["transport_retries"]
        <= RESOURCES["transport_max_retries_per_request"]
        and result["retry_attribution_valid"]
    )
    result["execution_identity_valid"] = (
        result["surface_identity_valid"]
        and result["budget_identity_valid"]
        and result["retry_treatment_valid"]
    )
    result["surface_valid"] = (
        result["surface_totals_valid"] and result["surface_identity_valid"]
    )
    result["valid"] = (
        requests["started"] >= 1
        and requests["started"] == requests["completed"]
        and requests["in_flight"] == 0
        and result["complete"] is True
        and result["usage_complete"] is True
        and result["billing_unknown"] is False
        and result["billing_unknown_attempts"] == 0
        and result["unpriced"] is False
        and result["sealed"] is True
        and result["usage_missing_responses"] == 0
        and result["incomplete_responses"] == 0
        and result["unpriced_usage_responses"] == 0
        and result["records_after_seal"] == 0
        and result["surface_totals_valid"]
        and result["retry_attribution_valid"]
    )
    result["requests"]["physical_attempts"] = result["requests"]["started"]
    return result


def latest_revision(workspace_state: Any) -> str | None:
    if not isinstance(workspace_state, dict):
        return None
    revision = workspace_state.get("revision")
    if isinstance(revision, dict) and revision.get("status") == "known":
        value = revision.get("sha256")
        return value if isinstance(value, str) else None
    return None


def known_revision_shape(revision: Any) -> bool:
    return bool(
        isinstance(revision, dict)
        and set(revision) == {"status", "sha256"}
        and revision.get("status") == "known"
        and isinstance(revision.get("sha256"), str)
        and revision["sha256"].strip()
    )


def nonempty_id(value: Any) -> bool:
    return isinstance(value, str) and bool(value.strip())


def unsigned_integer(value: Any) -> bool:
    return type(value) is int and value >= 0


def verifier_artifacts_available(
    outcome: Any,
    expected_spec: dict[str, Any],
    expected_verdict: str,
    expected_artifact_ids: Any,
) -> bool:
    if not isinstance(outcome, dict) or not isinstance(expected_artifact_ids, list):
        return False
    artifact_ids = expected_artifact_ids
    if (
        not artifact_ids
        or any(not isinstance(value, str) or not value for value in artifact_ids)
        or len(set(artifact_ids)) != len(artifact_ids)
    ):
        return False
    observation = outcome.get("verifier_observation")
    evidence = outcome.get("evidence")
    artifacts = outcome.get("artifacts")
    if (
        not isinstance(observation, dict)
        or set(observation)
        != {"spec", "verdict", "workspace_revision", "artifact_ids"}
        or not isinstance(evidence, dict)
        or not isinstance(artifacts, list)
        or evidence.get("status") != "produced"
        or evidence.get("references") != artifact_ids
        or observation.get("artifact_ids") != artifact_ids
        or observation.get("spec") != expected_spec
        or observation.get("verdict") != expected_verdict
        or not known_revision_shape(observation.get("workspace_revision"))
        or outcome.get("workspace_revision")
        != latest_revision({"revision": observation.get("workspace_revision")})
    ):
        return False
    available_ids = [
        artifact.get("id")
        for artifact in artifacts
        if isinstance(artifact, dict) and artifact.get("status") == "available"
    ]
    if available_ids != artifact_ids:
        return False
    for artifact_id in artifact_ids:
        matches = [
            artifact
            for artifact in artifacts
            if isinstance(artifact, dict) and artifact.get("id") == artifact_id
        ]
        if len(matches) != 1:
            return False
        artifact = matches[0]
        content = artifact.get("inline_content")
        if not isinstance(content, dict):
            return False
        encoded = canonical_bytes(content)
        digest = sha256_bytes(encoded)
        if (
            artifact.get("status") != "available"
            or set(content)
            != {"summary", "verifier", "verdict", "workspace_revision"}
            or artifact.get("sha256") != digest
            or artifact.get("id") != f"verification-evidence:{digest}"
            or artifact.get("media_type")
            != "application/vnd.codewhale.verification+json"
            or not unsigned_integer(artifact.get("byte_len"))
            or artifact["byte_len"] != len(encoded)
            or not isinstance(content.get("summary"), str)
            or not content["summary"].strip()
            or "\0" in content["summary"]
            or content.get("verifier") != expected_spec
            or content.get("verdict") != expected_verdict
            or content.get("workspace_revision")
            != observation.get("workspace_revision")
            or not known_revision_shape(content.get("workspace_revision"))
        ):
            return False
    return True


def verifier_artifact_closure_valid(
    outcome: Any,
    expected_spec: dict[str, Any],
    expected_verdict: str,
    expected_artifact_ids: Any,
) -> bool:
    return bool(
        isinstance(outcome, dict)
        and tool_outcome_shape_valid(outcome)
        and outcome.get("invocation") == "accepted"
        and outcome.get("transport") == "succeeded"
        and outcome.get("operation")
        == ("succeeded" if expected_verdict == "passed" else "failed")
        and (
            expected_verdict != "passed"
            or outcome.get("retry") == "not_needed"
        )
        and outcome.get("retry")
        in {"not_needed", "after_correction", "safe", "unsafe", "not_retryable"}
        and outcome.get("side_effect") != "applied"
        and verifier_artifacts_available(
            outcome,
            expected_spec,
            expected_verdict,
            expected_artifact_ids,
        )
    )


def tool_outcome_shape_valid(outcome: Any) -> bool:
    if not isinstance(outcome, dict):
        return False
    invocation = outcome.get("invocation")
    transport = outcome.get("transport")
    operation = outcome.get("operation")
    side_effect = outcome.get("side_effect")
    retry = outcome.get("retry")
    if (
        invocation not in {"accepted", "rejected"}
        or transport not in {"not_started", "succeeded", "failed", "indeterminate"}
        or operation
        not in {"not_started", "succeeded", "failed", "cancelled", "indeterminate"}
        or side_effect
        not in {"not_applicable", "not_applied", "applied", "indeterminate"}
        or retry
        not in {"not_needed", "after_correction", "safe", "unsafe", "not_retryable"}
    ):
        return False
    if invocation == "rejected" and (
        transport != "not_started"
        or operation != "not_started"
        or side_effect != "not_applied"
    ):
        return False
    if transport != "succeeded" and operation == "succeeded":
        return False
    if operation == "succeeded" and retry != "not_needed":
        return False
    if retry == "safe" and side_effect not in {"not_applicable", "not_applied"}:
        return False
    return True


def event_ledger_valid(events: list[dict[str, Any]], run: dict[str, Any]) -> bool:
    event_ids = [stored.get("event_id") for stored in events]
    schema_versions = {stored.get("schema_version") for stored in events}
    created_values = event_values(events, "run_created")
    request = (
        created_values[0].get("request", {}) if len(created_values) == 1 else {}
    )
    terminal = event_values(events, "terminal")
    outcome = terminal[0].get("outcome", {}) if len(terminal) == 1 else {}
    actor = request.get("actor", {})
    expected_root_actor = run.get("parent_run_id") is None
    actor_valid = (
        actor == {"kind": "root", "depth": 0}
        if expected_root_actor
        else actor.get("kind") == "child"
        and unsigned_integer(actor.get("depth"))
        and actor["depth"] > 0
    )
    return bool(
        events
        and event_kind(events[0]) == "run_created"
        and event_kind(events[-1]) == "terminal"
        and len(created_values) == 1
        and len(terminal) == 1
        and len(schema_versions) == 1
        and next(iter(schema_versions)) in {13, 14}
        and all(nonempty_id(value) for value in event_ids)
        and len(set(event_ids)) == len(event_ids)
        and all(unsigned_integer(stored.get("sequence")) for stored in events)
        and [stored.get("sequence") for stored in events]
        == list(range(1, len(events) + 1))
        and all(stored.get("run_id") == run.get("run_id") for stored in events)
        and run.get("last_sequence") == len(events)
        and request.get("run_id") == run.get("run_id")
        and actor_valid
        and request.get("task_contract", {}).get("generation_id")
        == run.get("run_id")
        and request.get("parent_run_id") == run.get("parent_run_id")
        and request.get("continued_from_run_id")
        == run.get("continued_from_run_id")
        and request.get("model") == run.get("model")
        and request.get("task_contract") == run.get("task_contract")
        and request.get("environment", {}).get("workspace")
        == run.get("workspace")
        and outcome.get("run_id") == run.get("run_id")
        and outcome.get("parent_run_id") == run.get("parent_run_id")
        and outcome.get("terminal") == run.get("terminal")
        and outcome.get("accounting") == run.get("accounting")
        and run.get("usage") == run.get("accounting", {}).get("usage")
        and outcome.get("runtime_model_requests") == run.get("runtime_model_requests")
        and outcome.get("runtime_retries") == run.get("runtime_retries")
        and outcome.get("tool_calls") == run.get("tool_calls")
    )


def run_terminal_evidence(
    events: list[dict[str, Any]],
    run: dict[str, Any],
) -> dict[str, Any]:
    terminals = event_values(events, "terminal")
    outcome = terminals[0].get("outcome", {}) if len(terminals) == 1 else {}
    run_projection = {
        "run_id": run.get("run_id"),
        "parent_run_id": run.get("parent_run_id"),
        "terminal": run.get("terminal"),
        "accounting": run.get("accounting"),
        "runtime_model_requests": run.get("runtime_model_requests"),
        "runtime_retries": run.get("runtime_retries"),
        "tool_calls": run.get("tool_calls"),
    }
    event_projection = {
        field: outcome.get(field)
        for field in (
            "run_id",
            "parent_run_id",
            "terminal",
            "accounting",
            "runtime_model_requests",
            "runtime_retries",
            "tool_calls",
        )
    }
    return {
        "events_sha256": canonical_hash(events),
        "run_view_sha256": canonical_hash(run),
        "terminal_outcome_sha256": canonical_hash(outcome),
        "run_terminal_projection_sha256": canonical_hash(run_projection),
        "event_terminal_projection_sha256": canonical_hash(event_projection),
        "projection_equal": run_projection == event_projection,
    }


def rejection_transition(cause: Any) -> str | None:
    if cause == "verifier_failed":
        return "effective_workspace_mutation"
    if cause == "verifier_spec_mismatch":
        return "host_verifier_contract_repair"
    if cause == "evidence_lineage_unavailable":
        return "evidence_lineage_repair"
    if cause in {
        "verifier_observation_missing",
        "verifier_workspace_unstable",
        "verifier_artifact_unavailable",
        "verifier_incomplete",
        "verifier_outcome_inconsistent",
        "evidence_receipt_invalid",
    }:
        return "host_verifier_execution_repair"
    return None


def stored_workspace_state(stored: dict[str, Any]) -> dict[str, Any] | None:
    event = stored.get("event", {})
    kind = event_kind(stored)
    value = None
    if kind == "tool_outcome_committed":
        value = event.get("workspace_state")
    elif kind == "workspace_observed":
        value = event.get("workspace_state")
    elif kind == "host_verification_committed":
        value = event.get("workspace_state_after")
    elif kind == "agent_integration_committed":
        value = event.get("root_workspace_state_after")
    return value if isinstance(value, dict) else None


def latest_workspace_before(
    events: list[dict[str, Any]],
    sequence: int,
) -> dict[str, Any] | None:
    values = [
        (stored["sequence"], workspace_state)
        for stored in events
        if unsigned_integer(stored.get("sequence"))
        and stored["sequence"] < sequence
        and (workspace_state := stored_workspace_state(stored)) is not None
    ]
    return max(values, key=lambda value: value[0])[1] if values else None


def tool_lifecycle_valid(
    events: list[dict[str, Any]],
    commit_sequence: int,
    commit: dict[str, Any],
    allowed_names: set[str],
) -> bool:
    operation_id = commit.get("operation_id")
    name = commit.get("name")
    if not nonempty_id(operation_id) or name not in allowed_names:
        return False
    prepared = [
        (stored.get("sequence"), stored["event"])
        for stored in events
        if event_kind(stored) == "tool_prepared"
        and stored["event"].get("operation_id") == operation_id
    ]
    started = [
        stored.get("sequence")
        for stored in events
        if event_kind(stored) == "tool_execution_started"
        and stored["event"].get("operation_id") == operation_id
    ]
    return bool(
        len(prepared) == 1
        and len(started) == 1
        and unsigned_integer(prepared[0][0])
        and unsigned_integer(started[0])
        and prepared[0][0] < started[0] < commit_sequence
        and prepared[0][1].get("workspace_access") == "may_write"
        and prepared[0][1].get("invocation", {}).get("name") == name
        and prepared[0][1].get("invocation", {}).get("call_id")
        == commit.get("call_id")
    )


def host_rejection_cause(
    prepared: dict[str, Any],
    commit: dict[str, Any],
) -> str | None:
    outcome = commit.get("outcome")
    if not tool_outcome_shape_valid(outcome):
        return None
    observation = outcome.get("verifier_observation") if isinstance(outcome, dict) else None
    if not isinstance(observation, dict):
        return "verifier_observation_missing"
    verifier = prepared.get("verifier")
    if observation.get("spec") != verifier:
        return "verifier_spec_mismatch"
    before = prepared.get("workspace_state_before")
    after = commit.get("workspace_state_after")
    observed_revision = observation.get("workspace_revision")
    stable = bool(
        isinstance(outcome, dict)
        and outcome.get("side_effect") != "applied"
        and latest_revision(before) is not None
        and latest_revision(before) == latest_revision(after)
        and latest_revision(after)
        == latest_revision({"revision": observed_revision})
        and outcome.get("workspace_revision") == latest_revision(after)
    )
    if not stable:
        return "verifier_workspace_unstable"
    verdict = observation.get("verdict")
    artifact_ids = observation.get("artifact_ids")
    if verdict not in {"passed", "failed", "partial"} or not verifier_artifacts_available(
        outcome,
        verifier,
        verdict,
        artifact_ids,
    ):
        return "verifier_artifact_unavailable"
    if (
        verdict == "passed"
        and outcome.get("invocation") == "accepted"
        and outcome.get("transport") == "succeeded"
        and outcome.get("operation") == "succeeded"
    ):
        return None
    if (
        verdict == "failed"
        and outcome.get("invocation") == "accepted"
        and outcome.get("transport") == "succeeded"
        and outcome.get("operation") == "failed"
    ):
        return "verifier_failed"
    if verdict == "partial":
        return "verifier_incomplete"
    return "verifier_outcome_inconsistent"


def effective_mutation_between(
    events: list[dict[str, Any]],
    after_sequence: int,
    before_sequence: int,
) -> bool:
    for stored in events:
        sequence = stored.get("sequence")
        if (
            event_kind(stored) != "tool_outcome_committed"
            or not unsigned_integer(sequence)
            or not after_sequence < sequence < before_sequence
        ):
            continue
        event = stored["event"]
        outcome = event.get("outcome", {})
        workspace_before = latest_workspace_before(events, sequence)
        workspace_after = event.get("workspace_state")
        if (
            event.get("name") in {"apply_patch", "edit_file"}
            and isinstance(outcome, dict)
            and tool_outcome_shape_valid(outcome)
            and outcome.get("invocation") == "accepted"
            and outcome.get("transport") == "succeeded"
            and outcome.get("side_effect") == "applied"
            and isinstance(workspace_before, dict)
            and isinstance(workspace_after, dict)
            and unsigned_integer(workspace_before.get("generation"))
            and unsigned_integer(workspace_after.get("generation"))
            and workspace_after["generation"] == workspace_before["generation"] + 1
            and latest_revision(workspace_before) is not None
            and latest_revision(workspace_before) != latest_revision(workspace_after)
            and tool_lifecycle_valid(
                events,
                sequence,
                event,
                {"apply_patch", "edit_file"},
            )
        ):
            return True
    return False


def t3_initial_tool_failure_valid(
    events: list[dict[str, Any]],
    frozen: dict[str, Any],
    before_sequence: int,
) -> bool:
    failures = []
    mutations = []
    for stored in events:
        sequence = stored.get("sequence")
        if (
            event_kind(stored) != "tool_outcome_committed"
            or not unsigned_integer(sequence)
            or sequence >= before_sequence
        ):
            continue
        event = stored["event"]
        outcome = event.get("outcome", {})
        observation = (
            outcome.get("verifier_observation", {})
            if isinstance(outcome, dict)
            else {}
        )
        if (
            event.get("name") == frozen.get("verifier_id")
            and verifier_artifact_closure_valid(
                outcome,
                frozen,
                "failed",
                observation.get("artifact_ids"),
            )
            and tool_lifecycle_valid(
                events,
                sequence,
                event,
                {frozen.get("verifier_id")},
            )
            and any(
                stored["event"].get("invocation", {})
                .get("arguments", {})
                .get("parsed")
                == {"verifier_id": MANIFEST["tasks"]["t3"]["acceptance_id"]}
                for stored in events
                if event_kind(stored) == "tool_prepared"
                and stored["event"].get("operation_id")
                == event.get("operation_id")
            )
        ):
            failures.append(sequence)
        workspace_before = latest_workspace_before(events, sequence)
        workspace_after = event.get("workspace_state")
        if (
            event.get("name") in {"apply_patch", "edit_file"}
            and isinstance(outcome, dict)
            and tool_outcome_shape_valid(outcome)
            and outcome.get("invocation") == "accepted"
            and outcome.get("transport") == "succeeded"
            and outcome.get("side_effect") == "applied"
            and latest_revision(workspace_before) is not None
            and latest_revision(workspace_before) != latest_revision(workspace_after)
            and tool_lifecycle_valid(
                events,
                sequence,
                event,
                {"apply_patch", "edit_file"},
            )
        ):
            mutations.append(sequence)
    return bool(failures and mutations and min(failures) < min(mutations))


def evidence_lineage_valid(
    task_id: str,
    lineage: Any,
    events: list[dict[str, Any]],
    final_commit_sequence: int,
    final_workspace: Any,
    frozen: dict[str, Any],
) -> bool:
    policy = MANIFEST["tasks"][task_id]["evidence_policy"]
    if policy == "latest_pass":
        return lineage == {"policy": "latest_pass"}
    if policy != "failed_write_pass" or not isinstance(lineage, dict):
        return False
    failure = lineage.get("failure")
    mutation = lineage.get("mutation")
    if (
        lineage.get("policy") != "failed_write_pass"
        or not isinstance(failure, dict)
        or not isinstance(mutation, dict)
        or not isinstance(final_workspace, dict)
    ):
        return False
    source = failure.get("source")
    failure_workspace = failure.get("workspace_state")
    mutation_before = mutation.get("workspace_state_before")
    mutation_after = mutation.get("workspace_state_after")
    if (
        not isinstance(source, dict)
        or source.get("kind") not in {"tool", "host"}
        or not nonempty_id(
            source.get(
                "operation_id" if source.get("kind") == "tool" else "verification_id"
            )
        )
        or not nonempty_id(mutation.get("operation_id"))
        or source.get("operation_id") == mutation.get("operation_id")
        or latest_revision(failure_workspace) is None
        or latest_revision(mutation_before) is None
        or latest_revision(mutation_after) is None
        or latest_revision(final_workspace) is None
        or latest_revision(mutation_before) == latest_revision(mutation_after)
        or latest_revision(failure_workspace) == latest_revision(final_workspace)
        or latest_revision(mutation_after) != latest_revision(final_workspace)
        or not unsigned_integer(failure_workspace.get("generation"))
        or not unsigned_integer(mutation_before.get("generation"))
        or not unsigned_integer(mutation_after.get("generation"))
        or not unsigned_integer(final_workspace.get("generation"))
        or failure_workspace["generation"] > mutation_before["generation"]
        or mutation_after["generation"] != mutation_before["generation"] + 1
        or final_workspace["generation"] <= mutation_after["generation"]
    ):
        return False
    failure_matches = []
    mutation_matches = []
    for stored in events:
        if event_kind(stored) != "tool_outcome_committed":
            continue
        event = stored["event"]
        sequence = stored.get("sequence")
        if (
            source.get("kind") == "tool"
            and event.get("operation_id") == source.get("operation_id")
        ):
            outcome = event.get("outcome")
            if (
                event.get("name") == frozen.get("verifier_id")
                and event.get("workspace_state") == failure_workspace
                and unsigned_integer(sequence)
                and tool_lifecycle_valid(
                    events,
                    sequence,
                    event,
                    {frozen.get("verifier_id")},
                )
                and any(
                    stored["event"].get("invocation", {})
                    .get("arguments", {})
                    .get("parsed")
                    == {"verifier_id": MANIFEST["tasks"][task_id]["acceptance_id"]}
                    for stored in events
                    if event_kind(stored) == "tool_prepared"
                    and stored["event"].get("operation_id")
                    == event.get("operation_id")
                )
                and verifier_artifact_closure_valid(
                    outcome,
                    frozen,
                    "failed",
                    failure.get("artifact_ids"),
                )
            ):
                failure_matches.append(sequence)
        if event.get("operation_id") == mutation["operation_id"]:
            outcome = event.get("outcome", {})
            if (
                isinstance(outcome, dict)
                and tool_outcome_shape_valid(outcome)
                and outcome.get("invocation") == "accepted"
                and outcome.get("transport") == "succeeded"
                and outcome.get("side_effect") == "applied"
                and event.get("workspace_state") == mutation_after
                and event.get("name") in {"apply_patch", "edit_file"}
                and unsigned_integer(sequence)
                and tool_lifecycle_valid(
                    events,
                    sequence,
                    event,
                    {"apply_patch", "edit_file"},
                )
            ):
                mutation_matches.append(sequence)
    if source.get("kind") == "host":
        for sequence, event in [
            (stored.get("sequence"), stored["event"])
            for stored in events
            if event_kind(stored) == "host_verification_committed"
        ]:
            if event.get("verification_id") != source.get("verification_id"):
                continue
            outcome = event.get("outcome")
            matching_prepared = [
                prepared_event
                for _, prepared_event in [
                    (stored.get("sequence"), stored["event"])
                    for stored in events
                    if event_kind(stored) == "host_verification_prepared"
                ]
                if prepared_event.get("verification_id")
                == source.get("verification_id")
            ]
            candidate_id = (
                matching_prepared[0].get("candidate", {}).get("id")
                if len(matching_prepared) == 1
                else None
            )
            matching_rejections = [
                (stored.get("sequence"), stored["event"].get("rejection", {}))
                for stored in events
                if event_kind(stored) == "completion_rejected"
                and unsigned_integer(stored.get("sequence"))
                and stored["sequence"] > sequence
                and stored["event"].get("rejection", {}).get("candidate_id")
                == candidate_id
            ]
            if (
                len(matching_prepared) == 1
                and len(matching_rejections) == 1
                and event.get("receipt") is None
                and event.get("workspace_state_after") == failure_workspace
                and host_rejection_cause(matching_prepared[0], event)
                == "verifier_failed"
                and verifier_artifact_closure_valid(
                    outcome,
                    frozen,
                    "failed",
                    failure.get("artifact_ids"),
                )
                and matching_rejections[0][0] < final_commit_sequence
            ):
                failure_matches.append(sequence)
    if not (
        len(failure_matches) == 1
        and len(mutation_matches) == 1
        and isinstance(failure_matches[0], int)
        and isinstance(mutation_matches[0], int)
        and failure_matches[0] < mutation_matches[0] < final_commit_sequence
    ):
        return False
    mutation_sequence = mutation_matches[0]
    prior_workspace = latest_workspace_before(events, mutation_sequence)
    settled_after_mutation = [
        workspace_state
        for stored in events
        if unsigned_integer(stored.get("sequence"))
        and mutation_sequence < stored["sequence"] < final_commit_sequence
        and (workspace_state := stored_workspace_state(stored)) is not None
    ]
    return bool(
        prior_workspace == mutation_before
        and t3_initial_tool_failure_valid(events, frozen, final_commit_sequence)
        and all(
            latest_revision(workspace_state) == latest_revision(final_workspace)
            and unsigned_integer(workspace_state.get("generation"))
            and workspace_state["generation"] >= mutation_after["generation"]
            for workspace_state in settled_after_mutation
        )
    )


def verification_summary(task_id: str, events: list[dict[str, Any]], run: dict[str, Any]) -> dict[str, Any]:
    created = run_created(events)
    contract = created.get("task_contract", {})
    acceptance = contract.get("definition", {}).get("acceptance", [])
    frozen = acceptance[0].get("verifier", {}) if len(acceptance) == 1 else {}
    generation_id = contract.get("generation_id")
    acceptance_id = MANIFEST["tasks"][task_id]["acceptance_id"]
    proposals = [
        (stored.get("sequence"), stored["event"].get("candidate", {}))
        for stored in events
        if event_kind(stored) == "completion_proposed"
    ]
    prepared = [
        (stored.get("sequence"), stored["event"])
        for stored in events
        if event_kind(stored) == "host_verification_prepared"
    ]
    started = [
        (stored.get("sequence"), stored["event"])
        for stored in events
        if event_kind(stored) == "host_verification_started"
    ]
    commit_records = [
        (stored.get("sequence"), stored["event"])
        for stored in events
        if event_kind(stored) == "host_verification_committed"
    ]
    commits = [event for _, event in commit_records]
    receipt_records = [
        (sequence, event, event["receipt"])
        for sequence, event in commit_records
        if isinstance(event.get("receipt"), dict)
    ]
    receipts = [receipt for _, _, receipt in receipt_records]
    observed_specs = []
    verdicts = []
    revisions = []
    for event in commits:
        outcome = event.get("outcome", {})
        observation = outcome.get("verifier_observation", {}) if isinstance(outcome, dict) else {}
        spec = observation.get("spec")
        if isinstance(spec, dict):
            observed_specs.append(canonical_hash(spec))
        verdicts.append(observation.get("verdict"))
        revisions.append(latest_revision(event.get("workspace_state_after")))
    repeated_same_revision = 0
    for previous, current in zip(commits, commits[1:]):
        if previous.get("receipt") is None and latest_revision(previous.get("workspace_state_after")) == latest_revision(current.get("workspace_state_after")):
            repeated_same_revision += 1
    terminal_events = event_values(events, "terminal")
    terminal_outcome = (
        terminal_events[0].get("outcome", {}) if len(terminal_events) == 1 else {}
    )
    terminal = (
        terminal_outcome.get("terminal", {})
        if isinstance(terminal_outcome, dict)
        else {}
    )
    decision = terminal.get("decision", {}) if isinstance(terminal, dict) else {}
    receipt = receipts[0] if len(receipts) == 1 else {}
    receipt_sequence, receipt_commit = (
        (receipt_records[0][0], receipt_records[0][1])
        if len(receipt_records) == 1
        else (None, {})
    )
    receipt_revision = latest_revision(receipt.get("workspace_state")) if isinstance(receipt, dict) else None
    frozen_hash = canonical_hash(frozen) if frozen else None
    resolved_hash = canonical_hash(verifier_spec(task_id, resolved=True))
    chain_failures = []
    ledger_valid = event_ledger_valid(events, run)
    if not ledger_valid:
        chain_failures.append("event_ledger_mismatch")
    lifecycle_valid = True
    prepared_ids = [event.get("verification_id") for _, event in prepared]
    started_ids = [event.get("verification_id") for _, event in started]
    commit_ids = [event.get("verification_id") for _, event in commit_records]
    if (
        not nonempty_id(generation_id)
        or not all(nonempty_id(value) for value in prepared_ids)
        or not all(nonempty_id(value) for value in started_ids)
        or not all(nonempty_id(value) for value in commit_ids)
        or
        len(set(prepared_ids)) != len(prepared_ids)
        or len(set(started_ids)) != len(started_ids)
        or len(set(commit_ids)) != len(commit_ids)
        or set(prepared_ids) != set(started_ids)
        or set(prepared_ids) != set(commit_ids)
    ):
        lifecycle_valid = False
    proposal_by_id = {
        candidate.get("id"): (sequence, candidate)
        for sequence, candidate in proposals
        if isinstance(candidate, dict) and nonempty_id(candidate.get("id"))
    }
    if len(proposal_by_id) != len(proposals):
        lifecycle_valid = False
    prepared_candidate_ids = [
        event.get("candidate", {}).get("id") for _, event in prepared
    ]
    if (
        len(set(prepared_candidate_ids)) != len(prepared_candidate_ids)
        or set(prepared_candidate_ids) != set(proposal_by_id)
    ):
        lifecycle_valid = False
    for prepared_sequence, prepared_event in prepared:
        verification_id = prepared_event.get("verification_id")
        candidate = prepared_event.get("candidate", {})
        proposal = proposal_by_id.get(candidate.get("id")) if isinstance(candidate, dict) else None
        matching_started = [
            sequence
            for sequence, event in started
            if event.get("verification_id") == verification_id
        ]
        matching_committed = [
            sequence
            for sequence, event in commit_records
            if event.get("verification_id") == verification_id
        ]
        if not (
            proposal is not None
            and proposal[1] == candidate
            and candidate.get("generation_id") == generation_id
            and prepared_event.get("acceptance_id") == acceptance_id
            and prepared_event.get("verifier") == frozen
            and latest_workspace_before(events, prepared_sequence)
            == prepared_event.get("workspace_state_before")
            and len(matching_started) == 1
            and len(matching_committed) == 1
            and unsigned_integer(proposal[0])
            and unsigned_integer(prepared_sequence)
            and unsigned_integer(matching_started[0])
            and unsigned_integer(matching_committed[0])
            and proposal[0] < prepared_sequence < matching_started[0] < matching_committed[0]
        ):
            lifecycle_valid = False
    if not lifecycle_valid:
        chain_failures.append("host_lifecycle_mismatch")
    rejections = event_values(events, "completion_rejected")
    event_schemas = {stored.get("schema_version") for stored in events}
    rejection_schema = next(iter(event_schemas)) if len(event_schemas) == 1 else None
    rejection_valid = rejection_schema in {13, 14}
    for rejection_event in rejections:
        rejection = rejection_event.get("rejection", {})
        proposal = proposal_by_id.get(rejection.get("candidate_id"))
        common_valid = (
            isinstance(rejection, dict)
            and proposal is not None
            and rejection.get("unmet_acceptance_ids") == [acceptance_id]
            and isinstance(rejection.get("reason"), str)
            and bool(rejection["reason"].strip())
        )
        typed_valid = rejection_schema == 13 or (
            rejection_schema == 14
            and rejection.get("generation_id") == generation_id
            and rejection_transition(rejection.get("cause"))
            == rejection.get("required_transition")
        )
        if not common_valid or not typed_valid:
            rejection_valid = False
    for commit_sequence, commit in commit_records:
        if isinstance(commit.get("receipt"), dict):
            continue
        matching_prepared = [
            event
            for _, event in prepared
            if event.get("verification_id") == commit.get("verification_id")
        ]
        candidate_id = (
            matching_prepared[0].get("candidate", {}).get("id")
            if len(matching_prepared) == 1
            else None
        )
        matching_rejections = [
            (stored.get("sequence"), stored["event"].get("rejection", {}))
            for stored in events
            if event_kind(stored) == "completion_rejected"
            and stored["event"].get("rejection", {}).get("candidate_id")
            == candidate_id
        ]
        prepared_event = matching_prepared[0] if len(matching_prepared) == 1 else {}
        expected_cause = host_rejection_cause(prepared_event, commit)
        observed_cause = (
            matching_rejections[0][1].get("cause")
            if len(matching_rejections) == 1
            else None
        )
        next_prepared_sequence = (
            min(
                (
                    sequence
                    for sequence, _ in prepared
                    if unsigned_integer(sequence) and sequence > commit_sequence
                ),
                default=len(events) + 1,
            )
            if unsigned_integer(commit_sequence)
            else len(events) + 1
        )
        if not (
            len(matching_rejections) == 1
            and expected_cause == "verifier_failed"
            and (
                rejection_schema == 13
                or observed_cause == expected_cause
            )
            and unsigned_integer(commit_sequence)
            and unsigned_integer(matching_rejections[0][0])
            and commit_sequence < matching_rejections[0][0]
            and effective_mutation_between(
                events,
                matching_rejections[0][0],
                next_prepared_sequence,
            )
        ):
            rejection_valid = False
    final_candidate_id = decision.get("candidate_id") if isinstance(decision, dict) else None
    if any(
        event.get("rejection", {}).get("candidate_id") == final_candidate_id
        for event in rejections
    ):
        rejection_valid = False
    if not rejection_valid:
        chain_failures.append("completion_rejection_mismatch")
    final_chain_valid = False
    lineage_valid = False
    artifact_valid = False
    final_candidate = proposal_by_id.get(final_candidate_id)
    if (
        len(receipt_records) == 1
        and unsigned_integer(receipt_sequence)
        and isinstance(receipt_commit, dict)
        and isinstance(receipt, dict)
        and final_candidate is not None
    ):
        verification_id = receipt_commit.get("verification_id")
        candidate = final_candidate[1]
        matching_prepared = [
            (sequence, event)
            for sequence, event in prepared
            if event.get("verification_id") == verification_id
        ]
        receipt_ids = receipt.get("artifact_ids")
        committed_outcome = receipt_commit.get("outcome")
        observation = (
            committed_outcome.get("verifier_observation", {})
            if isinstance(committed_outcome, dict)
            else {}
        )
        workspace_before = (
            matching_prepared[0][1].get("workspace_state_before", {})
            if len(matching_prepared) == 1
            else {}
        )
        before_generation = (
            workspace_before.get("generation")
            if isinstance(workspace_before, dict)
            else None
        )
        receipt_workspace = receipt.get("workspace_state")
        receipt_generation = (
            receipt_workspace.get("generation")
            if isinstance(receipt_workspace, dict)
            else None
        )
        artifact_valid = verifier_artifact_closure_valid(
            committed_outcome,
            frozen,
            "passed",
            receipt_ids,
        ) and isinstance(receipt_workspace, dict) and observation.get(
            "workspace_revision"
        ) == receipt_workspace.get("revision")
        lineage_valid = evidence_lineage_valid(
            task_id,
            receipt.get("lineage"),
            events,
            receipt_sequence,
            receipt.get("workspace_state"),
            frozen,
        )
        final_chain_valid = bool(
            len(matching_prepared) == 1
            and candidate.get("generation_id") == generation_id
            and nonempty_id(candidate.get("id"))
            and nonempty_id(verification_id)
            and receipt.get("id") == f"receipt:{verification_id}"
            and nonempty_id(receipt.get("id"))
            and receipt.get("generation_id") == generation_id
            and receipt.get("acceptance_id") == acceptance_id
            and receipt.get("verification_id") == verification_id
            and receipt.get("verifier") == frozen
            and receipt_commit.get("workspace_state_after")
            == receipt.get("workspace_state")
            and matching_prepared[0][1].get("candidate") == candidate
            and terminal.get("message") == candidate.get("message")
            and unsigned_integer(before_generation)
            and unsigned_integer(receipt_generation)
            and before_generation + 1 == receipt_generation
            and latest_revision(workspace_before)
            == receipt_revision
            and decision
            == {
                "candidate_id": candidate.get("id"),
                "generation_id": generation_id,
                "workspace_state": receipt.get("workspace_state"),
                "satisfied": [
                    {
                        "kind": "evidence",
                        "acceptance_id": acceptance_id,
                        "receipt_id": receipt.get("id"),
                    }
                ],
            }
            and terminal_outcome.get("details", {}).get("summary")
            == terminal.get("message")
            and terminal_outcome.get("details", {}).get("evidence") == [receipt]
            and artifact_valid
            and lineage_valid
        )
    if not final_chain_valid:
        chain_failures.append("final_evidence_chain_mismatch")
    if not artifact_valid:
        chain_failures.append("artifact_closure_mismatch")
    if not lineage_valid:
        chain_failures.append("evidence_lineage_mismatch")
    valid = (
        terminal_state(run) == "completed"
        and len(receipts) == 1
        and frozen == verifier_spec(task_id, resolved=True)
        and observed_specs[-1:] == [resolved_hash]
        and verdicts[-1:] == ["passed"]
        and receipt.get("verifier") == frozen
        and receipt.get("acceptance_id") == acceptance_id
        and receipt_revision is not None
        and receipt_revision == revisions[-1]
        and decision.get("workspace_state") == receipt.get("workspace_state")
        and repeated_same_revision == 0
        and not chain_failures
    )
    rejection_facts = []
    for event in event_values(events, "completion_rejected"):
        rejection = event.get("rejection", {})
        rejection_facts.append(
            {
                "cause": rejection.get("cause"),
                "required_transition": rejection.get("required_transition"),
                "reason_sha256": sha256_bytes(str(rejection.get("reason", "")).encode()),
            }
        )
    verification_attempts = []
    for prepared_sequence, prepared_event in prepared:
        verification_id = prepared_event.get("verification_id")
        candidate = prepared_event.get("candidate", {})
        matching_started = [
            sequence
            for sequence, event in started
            if event.get("verification_id") == verification_id
        ]
        matching_committed = [
            (sequence, event)
            for sequence, event in commit_records
            if event.get("verification_id") == verification_id
        ]
        committed_sequence, committed = (
            matching_committed[0] if len(matching_committed) == 1 else (None, {})
        )
        outcome = committed.get("outcome", {})
        artifacts = outcome.get("artifacts", []) if isinstance(outcome, dict) else []
        attempt_receipt = committed.get("receipt")
        matching_rejection = [
            event.get("rejection")
            for event in rejections
            if event.get("rejection", {}).get("candidate_id")
            == candidate.get("id")
        ]
        verification_attempts.append(
            {
                "candidate_id": candidate.get("id"),
                "verification_id": verification_id,
                "prepared_sequence": prepared_sequence,
                "started_sequence": (
                    matching_started[0] if len(matching_started) == 1 else None
                ),
                "committed_sequence": committed_sequence,
                "candidate_sha256": canonical_hash(candidate),
                "verifier_sha256": canonical_hash(prepared_event.get("verifier")),
                "workspace_before_sha256": canonical_hash(
                    prepared_event.get("workspace_state_before")
                ),
                "workspace_after_sha256": canonical_hash(
                    committed.get("workspace_state_after")
                ),
                "outcome_sha256": canonical_hash(outcome),
                "artifact_ids_sha256": canonical_hash(
                    [
                        artifact.get("id")
                        for artifact in artifacts
                        if isinstance(artifact, dict)
                    ]
                ),
                "artifact_payloads_sha256": canonical_hash(
                    [
                        artifact.get("inline_content")
                        for artifact in artifacts
                        if isinstance(artifact, dict)
                    ]
                ),
                "receipt_sha256": canonical_hash(attempt_receipt),
                "lineage_sha256": canonical_hash(
                    attempt_receipt.get("lineage")
                    if isinstance(attempt_receipt, dict)
                    else None
                ),
                "rejection_sha256": canonical_hash(
                    matching_rejection[0] if len(matching_rejection) == 1 else None
                ),
            }
        )
    canonical_run = run_terminal_evidence(events, run)
    return {
        "valid": valid,
        "ledger_valid": ledger_valid,
        "verification_chain_valid": final_chain_valid,
        "artifact_closure_valid": artifact_valid,
        "lineage_valid": lineage_valid,
        "chain_failures": sorted(set(chain_failures)),
        "chain_identity": {
            "generation_id": generation_id,
            "rejection_schema": rejection_schema,
            "candidate_id": final_candidate_id,
            "verification_id": receipt_commit.get("verification_id"),
            "receipt_id": receipt.get("id"),
            "artifact_ids_sha256": canonical_hash(receipt.get("artifact_ids")),
            "lineage_sha256": canonical_hash(receipt.get("lineage")),
            "terminal_decision_sha256": canonical_hash(decision),
            "event_ledger_sha256": canonical_hash(
                [
                    [stored.get("sequence"), stored.get("event_id"), event_kind(stored)]
                    for stored in events
                ]
            ),
            **canonical_run,
        },
        "verification_attempts": verification_attempts,
        "run_created_contract_sha256": canonical_hash(contract),
        "run_created_task_definition_sha256": canonical_hash(
            contract.get("definition")
        ),
        "contract_spec_sha256": frozen_hash,
        "caller_spec_sha256": canonical_hash(verifier_spec(task_id, resolved=False)),
        "resolved_spec_sha256": resolved_hash,
        "observed_spec_sha256": observed_specs,
        "host_commit_count": len(commits),
        "receipt_count": len(receipts),
        "verdicts": verdicts,
        "repeated_same_revision": repeated_same_revision,
        "completion_proposals": len(proposals),
        "completion_rejections": len(rejections),
        "rejections": rejection_facts,
    }


def tool_summary(events: list[dict[str, Any]]) -> dict[str, Any]:
    names: list[str] = []
    argument_hashes: list[str] = []
    outcomes: Counter[str] = Counter()
    outcome_axes: dict[str, dict[str, Any]] = {}
    verifier_verdicts: list[str] = []
    for event in event_values(events, "tool_prepared"):
        invocation = event.get("invocation", {})
        name = invocation.get("name")
        if isinstance(name, str):
            names.append(name)
        argument_hashes.append(canonical_hash(invocation.get("arguments")))
    for event in event_values(events, "tool_outcome_committed"):
        outcome = event.get("outcome", {})
        name = event.get("name", "unknown")
        evidence = outcome.get("evidence", {})
        artifacts = outcome.get("artifacts", [])
        revision = outcome.get("workspace_revision")
        revision_bound = isinstance(revision, str)
        axes = {
            "name": name,
            "invocation": outcome.get("invocation"),
            "transport": outcome.get("transport"),
            "operation": outcome.get("operation"),
            "side_effect": outcome.get("side_effect"),
            "retry": outcome.get("retry"),
            "evidence": evidence.get("status") if isinstance(evidence, dict) else None,
            "artifact_states": sorted(
                artifact.get("status")
                for artifact in artifacts
                if isinstance(artifact, dict) and isinstance(artifact.get("status"), str)
            ),
            "revision_bound": revision_bound,
        }
        digest = canonical_hash(axes)
        outcome_axes[digest] = axes
        outcomes[digest] += 1
        observation = outcome.get("verifier_observation")
        if isinstance(observation, dict) and isinstance(observation.get("verdict"), str):
            verifier_verdicts.append(observation["verdict"])
    return {
        "names": names,
        "argument_sha256": argument_hashes,
        "outcomes": dict(sorted(outcomes.items())),
        "outcome_axes": {digest: outcome_axes[digest] for digest in sorted(outcome_axes)},
        "verifier_verdicts": verifier_verdicts,
    }


def request_identity(events: list[dict[str, Any]]) -> dict[str, Any]:
    requests = event_values(events, "model_request_prepared")
    request = requests[0].get("request", {}) if requests else {}
    system_prompt = request.get("system_prompt", {}) if request else {}
    blocks = system_prompt.get("blocks", []) if isinstance(system_prompt, dict) else []
    stable_blocks = [
        block
        for block in blocks
        if isinstance(block, dict) and block.get("cache_control") == "stable"
    ]
    return {
        "request_sha256": canonical_hash(request) if request else None,
        "system_prompt_sha256": canonical_hash(system_prompt) if request else None,
        "stable_system_prompt_sha256": canonical_hash(stable_blocks) if request else None,
        "volatile_system_prompt_blocks": len(blocks) - len(stable_blocks),
        "ordered_tools_sha256": canonical_hash(request.get("tools")) if request else None,
    }


def agent_execution_wall_time_ms(events: list[dict[str, Any]]) -> int | None:
    created = [event for event in events if event_kind(event) == "run_created"]
    terminal = [event for event in events if event_kind(event) == "terminal"]
    if len(created) != 1 or len(terminal) != 1:
        return None
    started = created[0].get("occurred_at_unix_ms")
    finished = terminal[0].get("occurred_at_unix_ms")
    if not isinstance(started, int) or not isinstance(finished, int) or finished < started:
        return None
    return finished - started


def is_false_success(
    terminal: str | None,
    behavioral_verified: bool,
    behavior_evidence_known: bool,
) -> bool:
    return (
        terminal == "completed"
        and behavior_evidence_known
        and not behavioral_verified
    )


def child_summary(
    client: Any,
    root_events: list[dict[str, Any]],
    suffix: str,
    expected_event_schema: int,
    deadline: float,
) -> dict[str, Any]:
    lifecycle_counts = {
        kind: len(event_values(root_events, kind))
        for kind in (
            "agent_task_prepared",
            "child_started",
            "agent_result_collected",
            "child_finished",
        )
    }
    prepared = [
        (stored.get("sequence"), stored["event"])
        for stored in root_events
        if event_kind(stored) == "agent_task_prepared"
    ]
    children = []
    for index, (prepared_sequence, event) in enumerate(prepared):
        task = event.get("task", {})
        child_id = task.get("child_run_id")
        require(isinstance(child_id, str), "child_id_missing")
        result = client.call(
            CANARY.query("get", child_id, f"m7-child-get-{suffix}-{index}"),
            timeout_seconds=remaining_timeout(deadline),
        )
        require(result.get("kind") == "run", "child_run_missing")
        child = result["run"]
        child_events = events(
            client,
            child_id,
            f"m7-child-events-{suffix}-{index}",
            expected_event_schema,
            deadline,
        )
        counts = dict(
            sorted(Counter(event_kind(stored) for stored in child_events).items())
        )
        canonical_evidence = run_terminal_evidence(child_events, child)
        lifecycle = child_lifecycle_evidence(
            root_events,
            prepared_sequence,
            task,
            child_events,
            child,
        )
        children.append(
            {
                "child_run_id_sha256": sha256_bytes(child_id.encode()),
                "call_id_sha256": sha256_bytes(str(task.get("call_id", "")).encode()),
                "parent_identity_sha256": canonical_hash(
                    [child.get("parent_run_id"), root_events[0].get("run_id")]
                ),
                "terminal": terminal_summary(child),
                "tool": tool_summary(child_events),
                "event_counts": counts,
                "event_ledger_valid": event_ledger_valid(child_events, child),
                "event_ledger_sha256": canonical_hash(
                    [
                        [stored.get("sequence"), stored.get("event_id"), event_kind(stored)]
                        for stored in child_events
                    ]
                ),
                "canonical_evidence": canonical_evidence,
                "lifecycle": lifecycle,
                "workspace_access": task.get("workspace", {}).get("access"),
            }
        )
    return {
        "count": len(children),
        "lifecycle_counts": lifecycle_counts,
        "children": children,
        "logical_model_requests": sum(
            child["event_counts"].get("model_request_prepared", 0)
            for child in children
        ),
        "tool_calls": sum(
            child["event_counts"].get("tool_prepared", 0) for child in children
        ),
    }


def child_lifecycle_evidence(
    root_events: list[dict[str, Any]],
    prepared_sequence: Any,
    task: dict[str, Any],
    child_events: list[dict[str, Any]],
    child: dict[str, Any],
) -> dict[str, Any]:
    root_id = root_events[0].get("run_id") if root_events else None
    root_request = run_created(root_events)
    child_request = run_created(child_events)
    task_id = task.get("task_id")
    call_id = task.get("call_id")
    child_id = task.get("child_run_id")
    terminal_records = [
        (stored.get("sequence"), stored["event"].get("outcome"))
        for stored in child_events
        if event_kind(stored) == "terminal"
    ]
    child_terminal = terminal_records[0][1] if len(terminal_records) == 1 else None
    started = [
        (stored.get("sequence"), stored["event"])
        for stored in root_events
        if event_kind(stored) == "child_started"
        and (
            stored["event"].get("task_id") == task_id
            or stored["event"].get("call_id") == call_id
            or stored["event"].get("child_run_id") == child_id
        )
    ]
    collected = [
        (stored.get("sequence"), stored["event"])
        for stored in root_events
        if event_kind(stored) == "agent_result_collected"
        and stored["event"].get("task_id") == task_id
    ]
    finished = [
        (stored.get("sequence"), stored["event"])
        for stored in root_events
        if event_kind(stored) == "child_finished"
        and (
            stored["event"].get("call_id") == call_id
            or stored["event"].get("outcome", {}).get("run_id") == child_id
        )
    ]
    started_sequence, started_event = started[0] if len(started) == 1 else (None, {})
    collected_sequence, collected_event = (
        collected[0] if len(collected) == 1 else (None, {})
    )
    finished_sequence, finished_event = finished[0] if len(finished) == 1 else (None, {})
    finished_outcome = finished_event.get("outcome", {})
    expected_handoff = None
    if isinstance(finished_outcome, dict) and nonempty_id(child_id):
        payload = json.dumps(
            {
                "run_id": finished_outcome.get("run_id"),
                "terminal": finished_outcome.get("terminal"),
                "summary": finished_outcome.get("details", {}).get("summary"),
            },
            ensure_ascii=False,
            separators=(",", ":"),
        )
        expected_handoff = (
            f'<codewhale:runtime_event kind="subagent_completion" '
            f'agent_id="{child_id}">\n{payload}\n</codewhale:runtime_event>'
        )
    handoff_consumed = any(
        unsigned_integer(stored.get("sequence"))
        and unsigned_integer(finished_sequence)
        and stored["sequence"] > finished_sequence
        and any(
            message
            == {"role": "user", "content": finished_event.get("handoff_content")}
            for message in stored["event"].get("request", {}).get("messages", [])
        )
        for stored in root_events
        if event_kind(stored) == "model_request_prepared"
    )
    identity_valid = bool(
        nonempty_id(root_id)
        and nonempty_id(task_id)
        and nonempty_id(call_id)
        and nonempty_id(child_id)
        and task.get("root_run_id") == root_id
        and task.get("parent_run_id") == root_id
        and task.get("task_contract", {}).get("generation_id") == child_id
        and root_request.get("actor") == {"kind": "root", "depth": 0}
        and child.get("run_id") == child_id
        and child.get("parent_run_id") == root_id
        and child_request.get("run_id") == child_id
        and child_request.get("parent_run_id") == root_id
        and child_request.get("agent_task") == task
        and child_request.get("task_contract") == task.get("task_contract")
        and child_request.get("tool_policy") == task.get("tool_policy")
        and child_request.get("limits") == task.get("limits")
        and child_request.get("deadline_unix_ms") == task.get("deadline_unix_ms")
        and child_request.get("environment", {}).get("workspace")
        == task.get("workspace", {}).get("root_workspace")
        and len(terminal_records) == 1
        and started_event.get("task_id") == task_id
        and started_event.get("call_id") == call_id
        and started_event.get("child_run_id") == child_id
        and unsigned_integer(started_event.get("depth"))
        and unsigned_integer(root_request.get("actor", {}).get("depth"))
        and started_event.get("depth") == root_request["actor"]["depth"] + 1
        and child_request.get("actor")
        == {"kind": "child", "depth": started_event.get("depth")}
        and collected_event.get("task_id") == task_id
        and collected_event.get("outcome") == child_terminal
        and finished_event.get("call_id") == call_id
        and finished_event.get("outcome") == child_terminal
        and finished_event.get("handoff_content") == expected_handoff
        and handoff_consumed
    )
    sequence_valid = bool(
        all(
            unsigned_integer(value)
            for value in (
                prepared_sequence,
                started_sequence,
                collected_sequence,
                finished_sequence,
                terminal_records[0][0] if len(terminal_records) == 1 else None,
            )
        )
        and prepared_sequence < started_sequence < collected_sequence < finished_sequence
    )
    return {
        "valid": identity_valid and sequence_valid,
        "identity_valid": identity_valid,
        "sequence_valid": sequence_valid,
        "handoff_consumed": handoff_consumed,
        "task_sha256": canonical_hash(task),
        "child_terminal_outcome_sha256": canonical_hash(child_terminal),
        "collected_outcome_sha256": canonical_hash(collected_event.get("outcome")),
        "finished_outcome_sha256": canonical_hash(finished_event.get("outcome")),
        "root_lifecycle_sha256": canonical_hash(
            [
                [prepared_sequence, task],
                [started_sequence, started_event],
                [collected_sequence, collected_event],
                [finished_sequence, finished_event],
            ]
        ),
    }


def child_expectation_valid(task_id: str, summary: dict[str, Any]) -> bool:
    expectation = MANIFEST["tasks"][task_id]["child_expectation"]
    if expectation in {"forbidden", "zero"}:
        return summary["count"] == 0 and all(
            count == 0 for count in summary["lifecycle_counts"].values()
        )
    if expectation == "exactly_one_read_only":
        if summary["count"] != 1:
            return False
        child = summary["children"][0]
        return (
            all(count == 1 for count in summary["lifecycle_counts"].values())
            and child["workspace_access"] == "read_only"
            and child["terminal"]["state"] == "completed"
            and child["event_ledger_valid"]
            and child["canonical_evidence"]["projection_equal"]
            and child["lifecycle"]["valid"]
            and all(name in MANIFEST["tool_policy"]["readonly_child_tools"] for name in child["tool"]["names"])
            and not any(name in {"apply_patch", "edit_file", "exec_shell", "run_tests", "run_verifiers"} for name in child["tool"]["names"])
        )
    return False


def parse_changed_files(porcelain: str) -> list[str]:
    paths = []
    for line in porcelain.splitlines():
        if not line:
            continue
        raw = line[3:]
        paths.append(raw.split(" -> ")[-1])
    return sorted(paths)


def changed_files(workspace: Path) -> list[str]:
    porcelain = run_git(workspace, "status", "--porcelain=v1", "--untracked-files=all")
    return parse_changed_files(porcelain)


def external_verifier(workspace: Path, deadline: float) -> dict[str, Any]:
    before = snapshot_tree(workspace)
    started = time.monotonic()
    environment = safe_env()
    environment["PYTHONDONTWRITEBYTECODE"] = "1"
    try:
        result = subprocess.run(
            ["/usr/bin/python3", "-I", "-B", "_eval_verifier.py", "."],
            cwd=workspace,
            env=environment,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=remaining_timeout(deadline),
            check=False,
        )
    except subprocess.TimeoutExpired as error:
        after = snapshot_tree(workspace)
        return {
            "passed": False,
            "completed": False,
            "timed_out": True,
            "returncode": None,
            "workspace_unchanged": before == after,
            "stdout_sha256": sha256_bytes(error.stdout or b""),
            "stderr_sha256": sha256_bytes(error.stderr or b""),
            "duration_ms": int((time.monotonic() - started) * 1000),
        }
    after = snapshot_tree(workspace)
    return {
        "passed": result.returncode == 0,
        "completed": True,
        "timed_out": False,
        "returncode": result.returncode,
        "workspace_unchanged": before == after,
        "stdout_sha256": sha256_bytes(result.stdout),
        "stderr_sha256": sha256_bytes(result.stderr),
        "duration_ms": int((time.monotonic() - started) * 1000),
    }


def state_schema(codewhale_home: Path, expected: int) -> dict[str, Any]:
    database = codewhale_home / "state.db"
    connection = sqlite3.connect(f"file:{database}?mode=ro", uri=True)
    try:
        row = connection.execute("PRAGMA user_version").fetchone()
    finally:
        connection.close()
    version = row[0] if row else None
    return {"valid": version == expected, "version": version, "sha256": file_hash(database)}


def wait_for_terminal(client: Any, process: Any, root_id: str, deadline: float, suffix: str) -> dict[str, Any]:
    poll = 0
    while True:
        result = client.call(
            CANARY.query("get", root_id, f"m7-get-{suffix}-{poll}"),
            timeout_seconds=remaining_timeout(deadline),
        )
        require(result.get("kind") == "run", "run_view_missing")
        run = result["run"]
        if run.get("terminal") is not None:
            return run
        require(process.poll() is None, "app_server_exited")
        require(time.monotonic() < deadline, "arm_deadline_exceeded")
        poll += 1
        time.sleep(0.2)


def execute_arm(
    task_id: str,
    variant: str,
    run_index: int,
    binary_source: Path,
    revision: str,
    key: str,
) -> dict[str, Any]:
    started = time.monotonic()
    deadline = started + RESOURCES["harness_wall_time_seconds"]
    evaluation_id = uuid.uuid4().hex
    schemas = MANIFEST["protocol_schemas"][variant]
    with tempfile.TemporaryDirectory(prefix=f"codewhale-m7-{task_id}-{variant}-") as raw:
        root = Path(raw)
        workspace = root / "workspace"
        base = materialize_fixture(task_id, workspace)
        state = root / "state"
        home, codewhale_home, xdg = state / "home", state / "codewhale", state / "xdg"
        for directory in (home, codewhale_home, xdg):
            directory.mkdir(parents=True)
        binary = root / "codewhale"
        shutil.copy2(binary_source, binary)
        binary.chmod(0o700)
        expected_binary = MANIFEST["binary_identities"][variant]
        require(
            file_hash(binary) == expected_binary["sha256"]
            and binary.stat().st_size == expected_binary["size_bytes"],
            "binary_changed_after_preflight",
        )
        secret = key.encode()
        credentialless = {
            **safe_env(),
            "HOME": str(home),
            "CODEWHALE_HOME": str(codewhale_home),
            "XDG_CONFIG_HOME": str(xdg),
        }
        identity = CANARY.probe_binary(binary, workspace, credentialless, revision)
        environment = {**credentialless, "DEEPSEEK_API_KEY": key}
        stderr_path = state / "app-server.stderr"
        with stderr_path.open("wb") as stderr_stream:
            process = subprocess.Popen(
                [
                    str(binary),
                    "--provider",
                    "deepseek",
                    "app-server",
                    "--stdio",
                    "--transport-max-retries",
                    str(RESOURCES["transport_max_retries_per_request"]),
                ],
                cwd=workspace,
                env=environment,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=stderr_stream,
                start_new_session=True,
            )
        environment["DEEPSEEK_API_KEY"] = ""
        client = None
        try:
            client = CANARY.Stdio(process, secret)
            suffix = f"{task_id}-{variant}-{run_index}-{evaluation_id}"
            response = client.call(
                start_command(task_id, workspace, f"m7-start-{suffix}"),
                timeout_seconds=remaining_timeout(deadline),
            )
            require(response.get("kind") == "run", "start_run_missing")
            root_id = response["run"].get("run_id")
            require(isinstance(root_id, str), "root_run_id_missing")
            run = wait_for_terminal(client, process, root_id, deadline, suffix)
            root_events = events(
                client,
                root_id,
                f"m7-events-{suffix}",
                schemas["runtime_event"],
                deadline,
            )
            children = child_summary(
                client,
                root_events,
                suffix,
                schemas["runtime_event"],
                deadline,
            )
            accounting = usage_summary(run)
            verification = verification_summary(task_id, root_events, run)
            tools = tool_summary(root_events)
            external = external_verifier(workspace, deadline)
            changed = changed_files(workspace)
            expected_changed = sorted(MANIFEST["tasks"][task_id]["expected_changed_files"])
            scope_valid = changed == expected_changed
            path_authority_valid = all(
                path in MANIFEST["tasks"][task_id]["allowed_paths"] for path in changed
            )
            tool_authority_valid = all(
                name in MANIFEST["tool_policy"]["root_tools"] for name in tools["names"]
            )
            child_authority_valid = all(
                child["workspace_access"] == "read_only" for child in children["children"]
            )
            child_valid = child_expectation_valid(task_id, children)
            temporal_valid = (
                task_id != "t3" or verification["lineage_valid"]
            )
            state_summary = state_schema(codewhale_home, schemas["state"])
            measurement_valid = (
                accounting["valid"]
                and accounting["execution_identity_valid"]
                and state_summary["valid"]
                and external["completed"]
                and verification["ledger_valid"]
                and all(
                    child["event_ledger_valid"] for child in children["children"]
                )
            )
            behavioral_verified = (
                verification["valid"]
                and external["passed"]
                and external["workspace_unchanged"]
                and scope_valid
                and child_valid
                and temporal_valid
                and path_authority_valid
                and tool_authority_valid
                and child_authority_valid
            )
            verified = behavioral_verified and measurement_valid
            false_success = is_false_success(
                terminal_state(run),
                behavioral_verified,
                external["completed"],
            )
            model_request = request_identity(root_events)
            root_event_counts = dict(
                sorted(Counter(event_kind(event) for event in root_events).items())
            )
            tree_event_counts = dict(root_event_counts)
            tree_event_counts["model_request_prepared"] = (
                root_event_counts.get("model_request_prepared", 0)
                + children["logical_model_requests"]
            )
            tree_event_counts["tool_prepared"] = (
                root_event_counts.get("tool_prepared", 0) + children["tool_calls"]
            )
            wall_time_ms = int((time.monotonic() - started) * 1000)
            result = {
                "task_id": task_id,
                "variant": variant,
                "run_index": run_index,
                "evaluation_id": evaluation_id,
                "revision": revision,
                "binary_sha256": file_hash(binary),
                "binary_identity": identity,
                "fixture_base_commit": base,
                "fixture_tree_sha256": fixture_hash(task_id),
                "task_definition_sha256": verification[
                    "run_created_task_definition_sha256"
                ],
                "model": MODEL,
                "api_surface": "standard_chat",
                "terminal": terminal_summary(run),
                "behavioral_verified": behavioral_verified,
                "behavior_evidence_known": external["completed"],
                "measurement_valid": measurement_valid,
                "verified_success": verified,
                "false_success": false_success,
                "accounting": accounting,
                "verification": verification,
                "tool": tools,
                "child": children,
                "child_expectation_valid": child_valid,
                "temporal_valid": temporal_valid,
                "external_verifier": external,
                "changed_files": changed,
                "final_workspace_tree_sha256": canonical_hash(
                    snapshot_tree(workspace)
                ),
                "git_diff_sha256": sha256_bytes(
                    run_git(workspace, "diff", "--binary", "HEAD").encode()
                ),
                "scope_valid": scope_valid,
                "path_authority_valid": path_authority_valid,
                "tool_authority_valid": tool_authority_valid,
                "child_authority_valid": child_authority_valid,
                "first_model_request_sha256": model_request["request_sha256"],
                "request_identity": model_request,
                "event_counts": root_event_counts,
                "tree_event_counts": tree_event_counts,
                "protocol_schemas": schemas,
                "state_schema": state_summary,
                "agent_execution_wall_time_ms": agent_execution_wall_time_ms(root_events),
                "wall_time_ms": wall_time_ms,
            }
        finally:
            if client is not None:
                client.close()
            CANARY.stop(process)
        stderr = stderr_path.read_bytes()
        require(secret not in stderr, "key_in_stderr")
        require(not CANARY.tree_contains(workspace, secret), "key_in_fixture")
        require(not CANARY.tree_contains(state, secret), "key_in_state")
        require(secret not in canonical_bytes(result), "key_in_result")
        return result


def formal_schedule() -> list[dict[str, Any]]:
    schedule = []
    for run_index, tasks in enumerate(MANIFEST["experiment"]["round_order"], start=1):
        for ordinal, task_id in enumerate(TASK_IDS, start=1):
            require(task_id in tasks, "schedule_task_missing")
        for task_id in tasks:
            ordinal = TASK_IDS.index(task_id) + 1
            first = "baseline" if (run_index + ordinal) % 2 == 0 else "candidate"
            order = (first, "candidate" if first == "baseline" else "baseline")
            for arm_position, variant in enumerate(order, start=1):
                schedule.append(
                    {
                        "task_id": task_id,
                        "run_index": run_index,
                        "variant": variant,
                        "arm_position": arm_position,
                    }
                )
    return schedule


def schedule_key(arm: dict[str, Any]) -> tuple[Any, ...]:
    return (
        arm.get("task_id"),
        arm.get("run_index"),
        arm.get("variant"),
        arm.get("arm_position"),
    )


def accounting_stop_code(accounting: dict[str, Any]) -> str | None:
    if accounting.get("billing_unknown") is True or accounting.get("billing_unknown_attempts", 0) > 0:
        return "aborted_unknown_billing"
    if accounting.get("unpriced") is True:
        return "aborted_unpriced"
    if accounting.get("sealed") is not True:
        return "aborted_unsealed"
    if not accounting.get("valid"):
        return "aborted_accounting_mismatch"
    return None


def accounting_cost_is_lower_bound(accounting: dict[str, Any]) -> bool:
    requests = accounting.get("requests", {})
    return bool(
        accounting.get("billing_unknown") is True
        or accounting.get("billing_unknown_attempts", 0) > 0
        or accounting.get("unpriced") is True
        or accounting.get("unpriced_usage_responses", 0) > 0
        or accounting.get("sealed") is not True
        or accounting.get("complete") is not True
        or accounting.get("usage_complete") is not True
        or accounting.get("usage_missing_responses", 0) > 0
        or accounting.get("incomplete_responses", 0) > 0
        or accounting.get("records_after_seal", 0) > 0
        or requests.get("started") != requests.get("completed")
        or requests.get("in_flight") != 0
        or accounting.get("surface_totals_valid") is False
    )


def arm_measurement_failures(arm: dict[str, Any]) -> list[str]:
    accounting = arm["accounting"]
    requests = accounting["requests"]
    events_count = arm["tree_event_counts"]
    failures: list[str] = []
    accounting_failure = accounting_stop_code(accounting)
    if accounting_failure is not None:
        failures.append(accounting_failure)
    if not accounting.get("execution_identity_valid"):
        failures.append("execution_identity_mismatch")
    if not arm["state_schema"]["valid"]:
        failures.append("state_schema_mismatch")
    if not arm["external_verifier"].get("completed"):
        failures.append("external_verifier_incomplete")
    if not arm.get("measurement_valid"):
        failures.append("measurement_invalid")
    if requests["physical_attempts"] > RESOURCES["max_physical_api_attempts_per_arm"]:
        failures.append("physical_api_attempt_limit_exceeded")
    if events_count.get("model_request_prepared", 0) > RESOURCES["max_logical_model_requests_per_arm"]:
        failures.append("logical_request_limit_exceeded")
    if accounting["runtime_retries"] > RESOURCES["max_runtime_retries_per_arm"]:
        failures.append("runtime_retry_limit_exceeded")
    if accounting["transport_retries"] > requests["started"] * RESOURCES["transport_max_retries_per_request"]:
        failures.append("transport_retry_limit_exceeded")
    if events_count.get("tool_prepared", 0) > RESOURCES["max_tool_calls_per_arm"]:
        failures.append("tool_call_limit_exceeded")
    if accounting["cost_nanousd"] > RESOURCES["max_known_cost_nanousd_per_arm"]:
        failures.append("per_arm_known_cost_limit")
    if arm["wall_time_ms"] > RESOURCES["harness_wall_time_seconds"] * 1000:
        failures.append("harness_wall_time_exceeded")
    agent_time = arm.get("agent_execution_wall_time_ms")
    if not isinstance(agent_time, int) or agent_time > RESOURCES["runtime_wall_time_seconds"] * 1000:
        failures.append("runtime_wall_time_invalid")
    identity = arm.get("request_identity", {})
    if not identity.get("stable_system_prompt_sha256") or not identity.get("ordered_tools_sha256"):
        failures.append("request_identity_missing")
    expected_binary = MANIFEST["binary_identities"][arm["variant"]]
    if (
        arm.get("revision") != expected_binary["revision"]
        or arm.get("binary_sha256") != expected_binary["sha256"]
        or arm.get("fixture_tree_sha256") != MANIFEST["tasks"][arm["task_id"]]["fixture_tree_sha256"]
        or arm.get("task_definition_sha256")
        != canonical_hash(runtime_task_definition(arm["task_id"], arm["variant"]))
        or arm.get("protocol_schemas") != MANIFEST["protocol_schemas"][arm["variant"]]
    ):
        failures.append("arm_identity_mismatch")
    return sorted(set(failures))


def arm_safety_failures(arm: dict[str, Any]) -> list[str]:
    failures = []
    if arm["false_success"]:
        failures.append("false_success")
    for field in ("path_authority_valid", "tool_authority_valid", "child_authority_valid"):
        if not arm[field]:
            failures.append(field)
    if arm["variant"] == "candidate":
        if arm.get("task_definition_sha256") != canonical_hash(
            runtime_task_definition(arm["task_id"], "candidate")
        ):
            failures.append("candidate_task_contract_mismatch")
        accounting = arm["accounting"]
        if (
            accounting.get("surface_identity_mismatch")
            or not accounting.get("budget_identity_valid")
            or not accounting.get("retry_treatment_valid")
        ):
            failures.append("candidate_execution_identity_mismatch")
        if not accounting.get("retry_attribution_valid"):
            failures.append("candidate_accounting_identity_mismatch")
        verification = arm["verification"]
        resolved = verification["resolved_spec_sha256"]
        observed = verification["observed_spec_sha256"]
        if verification["contract_spec_sha256"] != resolved:
            failures.append("candidate_contract_spec_mismatch")
        if observed and any(value != resolved for value in observed):
            failures.append("candidate_observed_spec_mismatch")
        if verification["repeated_same_revision"] != 0:
            failures.append("candidate_same_revision_repeat")
    return sorted(set(failures))


def treatment_identity_failures(arms: list[dict[str, Any]]) -> list[str]:
    failures = []
    for task_id in TASK_IDS:
        identities = {}
        for variant in VARIANTS:
            selected = [
                arm["request_identity"]
                for arm in arms
                if arm["task_id"] == task_id and arm["variant"] == variant
            ]
            for field in ("stable_system_prompt_sha256", "ordered_tools_sha256"):
                values = {identity.get(field) for identity in selected}
                if len(values) > 1:
                    failures.append(f"{task_id}_{variant}_{field}_unstable")
            identities[variant] = selected[0] if selected else None
        if all(identities.values()):
            for field in ("stable_system_prompt_sha256", "ordered_tools_sha256"):
                if identities["baseline"][field] != identities["candidate"][field]:
                    failures.append(f"{task_id}_{field}_treatment_mismatch")
    return sorted(failures)


def paired_efficiency(arms: list[dict[str, Any]]) -> dict[str, Any]:
    by_pair: dict[tuple[str, int], dict[str, dict[str, Any]]] = defaultdict(dict)
    for arm in arms:
        if arm["verified_success"]:
            by_pair[(arm["task_id"], arm["run_index"])][arm["variant"]] = arm
    pairs = [value for value in by_pair.values() if set(value) == set(VARIANTS)]
    per_task = Counter(
        baseline["task_id"]
        for pair in pairs
        for baseline in [pair["baseline"]]
    )
    sufficient = (
        len(pairs) >= MANIFEST["acceptance"]["efficiency_minimum_dual_success_pairs"]
        and all(
            per_task[task_id]
            >= MANIFEST["acceptance"]["efficiency_minimum_pairs_per_task"]
            for task_id in TASK_IDS
        )
    )
    metrics: dict[str, int] = {}
    thresholds_valid = True
    for name in ("requests", "tokens", "cost", "wall_time"):
        improvements: list[Fraction] = []
        for pair in pairs:
            baseline = pair["baseline"]
            candidate = pair["candidate"]
            if name == "requests":
                before = baseline["accounting"]["requests"]["physical_attempts"]
                after = candidate["accounting"]["requests"]["physical_attempts"]
            elif name == "tokens":
                before = baseline["accounting"]["usage"]["input_tokens"] + baseline["accounting"]["usage"]["output_tokens"]
                after = candidate["accounting"]["usage"]["input_tokens"] + candidate["accounting"]["usage"]["output_tokens"]
            elif name == "cost":
                before = baseline["accounting"]["cost_nanousd"]
                after = candidate["accounting"]["cost_nanousd"]
            else:
                before = baseline["agent_execution_wall_time_ms"]
                after = candidate["agent_execution_wall_time_ms"]
            if not isinstance(before, int) or not isinstance(after, int) or before <= 0:
                thresholds_valid = False
                continue
            improvements.append(Fraction(before - after, before))
        if len(improvements) != len(pairs) or not improvements:
            thresholds_valid = False
            metrics[name] = 0
            continue
        median = statistics.median(improvements)
        metrics[name] = median.numerator * 10_000 // median.denominator
        if median < Fraction(-MANIFEST["acceptance"]["efficiency_max_regression_percent"], 100):
            thresholds_valid = False
    benefit = any(
        basis_points >= MANIFEST["acceptance"]["efficiency_minimum_improvement_percent"] * 100
        for basis_points in metrics.values()
    )
    return {
        "dual_success_pairs": len(pairs),
        "pairs_per_task": dict(sorted(per_task.items())),
        "median_improvement_basis_points": metrics,
        "sufficient": sufficient,
        "qualifies": sufficient and thresholds_valid and benefit,
    }


def summarize(arms: list[dict[str, Any]], abort: dict[str, Any] | None = None) -> dict[str, Any]:
    by_variant: dict[str, Any] = {}
    for variant in VARIANTS:
        selected = [arm for arm in arms if arm["variant"] == variant]
        usage = {
            field: sum(arm["accounting"]["usage"][field] for arm in selected)
            for field in USAGE_FIELDS
        }
        by_variant[variant] = {
            "arms": len(selected),
            "verified_success": sum(bool(arm["verified_success"]) for arm in selected),
            "false_success": sum(bool(arm["false_success"]) for arm in selected),
            "terminal_states": dict(sorted(Counter(arm["terminal"]["state"] for arm in selected).items())),
            "completion_rejections": sum(arm["verification"]["completion_rejections"] for arm in selected),
            "same_revision_repeats": sum(arm["verification"]["repeated_same_revision"] for arm in selected),
            "physical_api_attempts": sum(
                arm["accounting"]["requests"]["physical_attempts"] for arm in selected
            ),
            "usage": usage,
            "cost_nanousd": sum(arm["accounting"]["cost_nanousd"] for arm in selected),
            "cost_nanocny": sum(arm["accounting"]["cost_nanocny"] for arm in selected),
            "wall_time_ms": sum(arm["wall_time_ms"] for arm in selected),
        }
    task_cells = {}
    for task_id in TASK_IDS:
        task_cells[task_id] = {
            variant: {
                "arms": len(selected := [arm for arm in arms if arm["task_id"] == task_id and arm["variant"] == variant]),
                "verified_success": sum(bool(arm["verified_success"]) for arm in selected),
                "false_success": sum(bool(arm["false_success"]) for arm in selected),
            }
            for variant in VARIANTS
        }
    delta = by_variant["candidate"]["verified_success"] - by_variant["baseline"]["verified_success"]
    expected_schedule = [schedule_key(arm) for arm in formal_schedule()]
    measurement_failures = {
        str(index): failures
        for index, arm in enumerate(arms)
        if (failures := arm_measurement_failures(arm))
    }
    safety_failures = {
        str(index): failures
        for index, arm in enumerate(arms)
        if (failures := arm_safety_failures(arm))
    }
    treatment_failures = treatment_identity_failures(arms)
    exact_schedule = [schedule_key(arm) for arm in arms] == expected_schedule
    measurement_complete = abort is None and exact_schedule and not measurement_failures
    hard_gates = {
        "false_success_zero": all(arm["false_success"] is False for arm in arms),
        "per_task_success_non_regression": all(
            task_cells[task]["candidate"]["verified_success"] >= task_cells[task]["baseline"]["verified_success"]
            for task in TASK_IDS
        ),
        "formal_schedule_exact": exact_schedule,
        "all_measurements_valid": not measurement_failures,
        "authority_and_safety_valid": not safety_failures,
        "treatment_identity_valid": not treatment_failures,
        "candidate_spec_exact": all(
            arm["verification"]["contract_spec_sha256"]
            == arm["verification"]["resolved_spec_sha256"]
            and all(
                value == arm["verification"]["resolved_spec_sha256"]
                for value in arm["verification"]["observed_spec_sha256"]
            )
            for arm in arms
            if arm["variant"] == "candidate"
        ),
        "candidate_same_revision_repeat_zero": by_variant["candidate"]["same_revision_repeats"] == 0,
    }
    efficiency = paired_efficiency(arms)
    product_metric_eligible = measurement_complete
    abort_safety_failure = bool(abort and abort.get("code") in SECRET_FAILURES)
    hard_gates["abort_safety_valid"] = not abort_safety_failure
    safety_valid = (
        hard_gates["false_success_zero"]
        and hard_gates["authority_and_safety_valid"]
        and hard_gates["treatment_identity_valid"]
        and hard_gates["candidate_spec_exact"]
        and hard_gates["candidate_same_revision_repeat_zero"]
        and hard_gates["abort_safety_valid"]
    )
    if not safety_valid:
        decision = "reject"
    elif not product_metric_eligible:
        decision = "hold"
    elif not hard_gates["per_task_success_non_regression"]:
        decision = "reject"
    elif delta >= 3:
        decision = "keep"
    elif delta == 0 and efficiency["qualifies"]:
        decision = "keep"
    else:
        decision = "reject"
    return {
        "by_variant": by_variant,
        "task_cells": task_cells,
        "verified_success_delta": delta,
        "measurement_failures": measurement_failures,
        "safety_failures": safety_failures,
        "treatment_identity_failures": treatment_failures,
        "paired_efficiency": efficiency,
        "hard_gates": hard_gates,
        "product_metric_eligible": product_metric_eligible,
        "decision": decision,
    }


def preflight_binary(variant: str, path: Path, revision: str) -> dict[str, Any]:
    expected = MANIFEST["binary_identities"][variant]
    try:
        metadata = path.lstat()
    except OSError as error:
        raise EvaluationError("binary_unavailable") from error
    require(
        stat.S_ISREG(metadata.st_mode)
        and not path.is_symlink()
        and os.access(path, os.X_OK),
        "binary_unavailable",
    )
    require(revision == expected["revision"], "binary_revision_mismatch")
    require(git_tree(revision) == expected["source_tree"], "source_tree_mismatch")
    require(file_hash(path) == expected["sha256"], "binary_sha256_mismatch")
    require(metadata.st_size == expected["size_bytes"], "binary_size_mismatch")
    with tempfile.TemporaryDirectory(prefix="codewhale-m7-probe-") as raw:
        workspace = Path(raw)
        identity = CANARY.probe_binary(path, workspace, safe_env(), revision)
        version_output = f"codewhale {identity['version']} ({identity['revision_prefix']})"
        require(version_output == expected["version"], "binary_version_mismatch")
        return {
            "revision": revision,
            "sha256": file_hash(path),
            "size_bytes": metadata.st_size,
            "source_tree": expected["source_tree"],
            "identity": identity,
        }


def preflight_fixtures() -> None:
    with tempfile.TemporaryDirectory(prefix="codewhale-m7-fixture-preflight-") as raw:
        root = Path(raw)
        for task_id in TASK_IDS:
            workspace = root / task_id
            base = materialize_fixture(task_id, workspace)
            require(
                base == MANIFEST["tasks"][task_id]["fixture_base_commit"],
                "fixture_base_commit_mismatch",
            )
            result = subprocess.run(
                ["/usr/bin/python3", "-I", "-B", "_eval_verifier.py", "."],
                cwd=workspace,
                env={**safe_env(), "PYTHONDONTWRITEBYTECODE": "1"},
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                timeout=30,
                check=False,
            )
            require(result.returncode != 0, "fixture_initial_verifier_unexpected_pass")


def preflight_diagnostic_evidence() -> None:
    evidence = MANIFEST["excluded_diagnostics"]["baseline_v2"]
    path = ROOT / evidence["path"]
    require(
        path.is_file()
        and file_hash(path) == evidence["sha256"]
        and evidence["product_metric_eligible"] is False
        and evidence["excluded_from_formal"] is True,
        "baseline_diagnostic_identity_mismatch",
    )


def validate_fresh_output(path: Path) -> Path:
    expanded = path.expanduser()
    partial = expanded.with_suffix(expanded.suffix + ".partial")
    require(
        not expanded.exists()
        and not expanded.is_symlink()
        and not partial.exists()
        and not partial.is_symlink(),
        "output_already_claimed",
    )
    raw_results_root = ROOT / "eval/results"
    try:
        raw_results_root.mkdir(mode=0o700, parents=False, exist_ok=True)
        metadata = raw_results_root.lstat()
    except OSError as error:
        raise EvaluationError("result_root_invalid") from error
    require(
        stat.S_ISDIR(metadata.st_mode) and not raw_results_root.is_symlink(),
        "result_root_invalid",
    )
    results_root = raw_results_root.resolve()
    resolved = expanded.resolve(strict=False)
    require(
        resolved.parent == results_root and resolved.suffix == ".json",
        "output_path_invalid",
    )
    return resolved


def expected_output_path(formal: bool) -> Path:
    identity = MANIFEST["output_identities"]["formal" if formal else "diagnostic"]
    relative = Path(identity["path"])
    require(
        not relative.is_absolute()
        and relative.parts[:2] == ("eval", "results")
        and len(relative.parts) == 3,
        "frozen_output_identity_invalid",
    )
    return (ROOT / relative).resolve(strict=False)


def validate_suite_output(path: Path, formal: bool) -> Path:
    resolved = validate_fresh_output(path)
    require(resolved == expected_output_path(formal), "frozen_output_identity_mismatch")
    return resolved


def claim_private_output(path: Path, value: Any) -> None:
    encoded = canonical_bytes(value) + b"\n"
    try:
        descriptor = os.open(
            path,
            os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0),
            0o600,
        )
    except OSError as error:
        raise EvaluationError("result_claim_failed") from error
    try:
        view = memoryview(encoded)
        while view:
            written = os.write(descriptor, view)
            require(written > 0, "result_claim_failed")
            view = view[written:]
        os.fsync(descriptor)
    finally:
        os.close(descriptor)
    directory = os.open(path.parent, os.O_RDONLY)
    try:
        os.fsync(directory)
    finally:
        os.close(directory)
    require(stat.S_IMODE(path.stat().st_mode) == 0o600, "result_mode_invalid")


def write_private_json(
    path: Path,
    value: Any,
    secret: bytes | None,
) -> None:
    encoded = canonical_bytes(value) + b"\n"
    if secret is not None:
        require(bool(secret) and secret not in encoded, "key_in_result")
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".partial")
    descriptor = os.open(
        temporary,
        os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0),
        0o600,
    )
    try:
        view = memoryview(encoded)
        while view:
            written = os.write(descriptor, view)
            require(written > 0, "result_write_failed")
            view = view[written:]
        os.fsync(descriptor)
    finally:
        os.close(descriptor)
    os.chmod(temporary, 0o600)
    os.replace(temporary, path)
    directory = os.open(path.parent, os.O_RDONLY)
    try:
        os.fsync(directory)
    finally:
        os.close(directory)
    require(stat.S_IMODE(path.stat().st_mode) == 0o600, "result_mode_invalid")


def suite_cost_limit(formal: bool) -> int:
    return RESOURCES[
        "formal_suite_known_cost_nanousd"
        if formal
        else "diagnostic_known_cost_nanousd"
    ]


def has_cost_headroom(known_cost_nanousd: int, maximum_cost_nanousd: int) -> bool:
    return (
        known_cost_nanousd + RESOURCES["max_known_cost_nanousd_per_arm"]
        <= maximum_cost_nanousd
    )


def run_suite(args: argparse.Namespace, *, formal: bool) -> dict[str, Any]:
    require(args.acknowledge_cost, "cost_acknowledgement_required")
    require(args.key_file is not None, "key_file_required")
    validate_frozen_manifest()
    output = validate_suite_output(args.output, formal)
    source_binaries = {
        "baseline": args.baseline_binary.expanduser().resolve(),
    }
    revisions = {"baseline": args.baseline_revision}
    if formal:
        require(
            args.candidate_binary is not None and args.candidate_revision is not None,
            "candidate_identity_required",
        )
        source_binaries["candidate"] = args.candidate_binary.expanduser().resolve()
        revisions["candidate"] = args.candidate_revision
        require(
            revisions["baseline"] != revisions["candidate"]
            and MANIFEST["binary_identities"]["baseline"]["sha256"]
            != MANIFEST["binary_identities"]["candidate"]["sha256"],
            "binary_treatments_not_distinct",
        )
    preflight_fixtures()
    preflight_diagnostic_evidence()
    schedule = formal_schedule() if formal else [
        {"task_id": task_id, "run_index": 0, "variant": "baseline", "arm_position": 1}
        for task_id in TASK_IDS
    ]
    maximum_cost = suite_cost_limit(formal)
    key = ""
    with tempfile.TemporaryDirectory(prefix="codewhale-m7-frozen-binaries-") as raw:
        frozen_root = Path(raw)
        binaries = {}
        identities = {}
        for variant, source in source_binaries.items():
            destination = frozen_root / f"{variant}-codewhale"
            try:
                shutil.copy2(source, destination)
                destination.chmod(0o500)
            except OSError as error:
                raise EvaluationError("binary_stage_failed") from error
            binaries[variant] = destination
            identities[variant] = preflight_binary(
                variant,
                destination,
                revisions[variant],
            )
        record = {
            "schema": RESULT_SCHEMA,
            "suite_id": MANIFEST["output_identities"][
                "formal" if formal else "diagnostic"
            ]["suite_id"],
            "mode": "formal" if formal else "baseline_diagnostic",
            "created_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "manifest_sha256": file_hash(MANIFEST_PATH),
            "manifest_content_sha256": manifest_content_hash(),
            "harness_sha256": file_hash(Path(__file__)),
            "canary_helper_sha256": file_hash(CANARY_PATH),
            "schedule_sha256": canonical_hash(schedule),
            "identities": identities,
            "model": MODEL,
            "api_surface": "standard_chat",
            "arms": [],
            "active_arm": None,
            "abort": None,
            "known_cost_is_lower_bound": False,
            "execution_state": "credentialless_preflight_complete",
        }
        claim_private_output(output, record)
        try:
            key = CANARY.read_key(args.key_file.expanduser())
        except CANARY.Failure as error:
            record["abort"] = {
                "code": error.code,
                "details_sha256": canonical_hash(error.details),
            }
            record["aggregate"] = None
            record["product_metric_eligible"] = False
            record["execution_state"] = "aborted_before_api"
            write_private_json(output, record, None)
            return record
        secret = key.encode()
        record["execution_state"] = "ready_after_key_read"
        write_private_json(output, record, secret)
        for scheduled in schedule:
            known_cost = sum(
                value["accounting"]["cost_nanousd"] for value in record["arms"]
            )
            if not has_cost_headroom(known_cost, maximum_cost):
                record["abort"] = {
                    "code": "known_cost_headroom_exhausted",
                    "known_cost_nanousd": known_cost,
                }
                write_private_json(output, record, secret)
                break
            record["active_arm"] = {
                **scheduled,
                "status": "reserved_before_api",
                "api_exposure": "possible_after_checkpoint",
            }
            record["known_cost_is_lower_bound"] = True
            record["execution_state"] = "running"
            write_private_json(output, record, secret)
            try:
                arm = execute_arm(
                    scheduled["task_id"],
                    scheduled["variant"],
                    scheduled["run_index"],
                    binaries[scheduled["variant"]],
                    revisions[scheduled["variant"]],
                    key,
                )
                arm["arm_position"] = scheduled["arm_position"]
                record["arms"].append(arm)
                record["active_arm"] = None
                record["known_cost_is_lower_bound"] = any(
                    accounting_cost_is_lower_bound(value["accounting"])
                    for value in record["arms"]
                )
                write_private_json(output, record, secret)

                measurement_failures = arm_measurement_failures(arm)
                safety_failures = arm_safety_failures(arm)
                treatment_failures = treatment_identity_failures(record["arms"])
                known_cost = sum(
                    value["accounting"]["cost_nanousd"] for value in record["arms"]
                )
                if safety_failures:
                    record["abort"] = {
                        "code": "safety_gate_failed",
                        "task_id": arm["task_id"],
                        "variant": arm["variant"],
                        "failures": safety_failures,
                    }
                elif treatment_failures:
                    record["abort"] = {
                        "code": "treatment_identity_mismatch",
                        "failures": treatment_failures,
                    }
                elif measurement_failures:
                    record["abort"] = {
                        "code": accounting_stop_code(arm["accounting"])
                        or measurement_failures[0],
                        "task_id": arm["task_id"],
                        "variant": arm["variant"],
                        "failures": measurement_failures,
                    }
                    record["known_cost_is_lower_bound"] = (
                        record["known_cost_is_lower_bound"]
                        or accounting_cost_is_lower_bound(arm["accounting"])
                    )
                elif known_cost > maximum_cost:
                    record["abort"] = {
                        "code": "known_cost_limit",
                        "known_cost_nanousd": known_cost,
                    }
                if record["abort"] is not None:
                    write_private_json(output, record, secret)
                    break
            except (EvaluationError, CANARY.Failure) as error:
                code = error.code
                record["active_arm"] = {
                    **scheduled,
                    "status": "failed_after_reservation",
                    "api_exposure": "possible",
                    "failure_code": code,
                    "details_sha256": canonical_hash(getattr(error, "details", {})),
                }
                record["abort"] = {
                    "code": code,
                    "details_sha256": canonical_hash(getattr(error, "details", {})),
                }
                record["known_cost_is_lower_bound"] = True
                write_private_json(output, record, secret)
                break
            except Exception as error:
                code = "unexpected_execution_failure"
                details = {"exception_type": type(error).__name__}
                record["active_arm"] = {
                    **scheduled,
                    "status": "failed_after_reservation",
                    "api_exposure": "possible",
                    "failure_code": code,
                    "details_sha256": canonical_hash(details),
                }
                record["abort"] = {
                    "code": code,
                    "details_sha256": canonical_hash(details),
                }
                record["known_cost_is_lower_bound"] = True
                write_private_json(output, record, secret)
                break
        record["aggregate"] = summarize(record["arms"], record["abort"]) if formal else None
        record["product_metric_eligible"] = bool(
            formal and record["aggregate"]["product_metric_eligible"]
        )
        record["execution_state"] = (
            "completed" if record["abort"] is None else "aborted"
        )
        write_private_json(output, record, secret)
        key = ""
        return record


def synthetic_arm(
    scheduled: dict[str, Any],
    *,
    verified: bool,
    false_success: bool = False,
    requests: int = 10,
    tokens: int = 10_000,
    cost_nanousd: int = 10_000_000,
    wall_time_ms: int = 1_000,
) -> dict[str, Any]:
    task_id = scheduled["task_id"]
    variant = scheduled["variant"]
    expected_binary = MANIFEST["binary_identities"][variant]
    usage = {field: 0 for field in USAGE_FIELDS}
    usage["input_tokens"] = tokens // 2
    usage["output_tokens"] = tokens - usage["input_tokens"]
    return {
        **scheduled,
        "revision": expected_binary["revision"],
        "binary_sha256": expected_binary["sha256"],
        "fixture_tree_sha256": MANIFEST["tasks"][task_id]["fixture_tree_sha256"],
        "task_definition_sha256": canonical_hash(
            runtime_task_definition(task_id, variant)
        ),
        "terminal": {"state": "completed" if verified or false_success else "blocked"},
        "behavioral_verified": verified,
        "behavior_evidence_known": True,
        "measurement_valid": True,
        "verified_success": verified,
        "false_success": false_success,
        "accounting": {
            "valid": True,
            "hard_request_limit": RESOURCES["max_physical_api_attempts_per_arm"],
            "requests": {
                "started": requests,
                "completed": requests,
                "in_flight": 0,
                "physical_attempts": requests,
            },
            "root": {"started": requests, "completed": requests, "in_flight": 0, "retries": 0},
            "child": {"started": 0, "completed": 0, "in_flight": 0, "retries": 0},
            "runtime_retries": 0,
            "transport_retries": 0,
            "billing_unknown_attempts": 0,
            "usage_responses": requests,
            "usage_missing_responses": 0,
            "incomplete_responses": 0,
            "unpriced_usage_responses": 0,
            "records_after_seal": 0,
            "usage": usage,
            "cost_nanousd": cost_nanousd,
            "cost_nanocny": 0,
            "complete": True,
            "usage_complete": True,
            "billing_unknown": False,
            "unpriced": False,
            "sealed": True,
            "surface_usage": [
                {
                    "surface": "standard_chat",
                    "model": MODEL,
                    "response_count": requests,
                    "usage_response_count": requests,
                    "usage": usage,
                    "cost_nanousd": cost_nanousd,
                    "cost_nanocny": 0,
                }
            ],
            "surface_totals_valid": True,
            "surface_identity_observed": True,
            "surface_identity_mismatch": False,
            "surface_identity_valid": True,
            "budget_identity_valid": True,
            "retry_attribution_valid": True,
            "retry_treatment_valid": True,
            "execution_identity_valid": True,
            "surface_valid": True,
        },
        "verification": {
            "completion_rejections": 0,
            "repeated_same_revision": 0,
            "contract_spec_sha256": "same",
            "resolved_spec_sha256": "same",
            "observed_spec_sha256": ["same"] if verified else [],
        },
        "external_verifier": {"completed": True},
        "state_schema": {"valid": True},
        "event_counts": {
            "model_request_prepared": requests,
            "tool_prepared": 1,
        },
        "tree_event_counts": {
            "model_request_prepared": requests,
            "tool_prepared": 1,
        },
        "protocol_schemas": MANIFEST["protocol_schemas"][variant],
        "request_identity": {
            "system_prompt_sha256": f"prompt-{task_id}",
            "stable_system_prompt_sha256": f"prompt-{task_id}",
            "ordered_tools_sha256": f"tools-{task_id}",
        },
        "path_authority_valid": True,
        "tool_authority_valid": True,
        "child_authority_valid": True,
        "agent_execution_wall_time_ms": wall_time_ms,
        "wall_time_ms": wall_time_ms,
    }


def synthetic_formal_arms(
    success: Any,
    metrics: Any | None = None,
) -> list[dict[str, Any]]:
    arms = []
    for scheduled in formal_schedule():
        values = metrics(scheduled) if metrics is not None else {}
        arms.append(
            synthetic_arm(
                scheduled,
                verified=bool(success(scheduled)),
                **values,
            )
        )
    return arms


def synthetic_verifier_artifact(
    spec: dict[str, Any],
    verdict: str,
    revision: dict[str, Any],
) -> tuple[str, dict[str, Any]]:
    content = {
        "summary": f"synthetic {verdict}",
        "verifier": spec,
        "verdict": verdict,
        "workspace_revision": revision,
    }
    encoded = canonical_bytes(content)
    digest = sha256_bytes(encoded)
    artifact_id = f"verification-evidence:{digest}"
    return artifact_id, {
        "id": artifact_id,
        "status": "available",
        "sha256": digest,
        "media_type": "application/vnd.codewhale.verification+json",
        "byte_len": len(encoded),
        "inline_content": content,
    }


def synthetic_verifier_outcome(
    spec: dict[str, Any],
    verdict: str,
    revision: dict[str, Any],
) -> tuple[dict[str, Any], list[str]]:
    artifact_id, artifact = synthetic_verifier_artifact(spec, verdict, revision)
    return {
        "invocation": "accepted",
        "transport": "succeeded",
        "operation": "succeeded" if verdict == "passed" else "failed",
        "side_effect": "not_applied",
        "retry": "not_needed",
        "evidence": {"status": "produced", "references": [artifact_id]},
        "artifacts": [artifact],
        "workspace_revision": revision["sha256"],
        "verifier_observation": {
            "spec": spec,
            "verdict": verdict,
            "workspace_revision": revision,
            "artifact_ids": [artifact_id],
        },
        "content": "synthetic verifier outcome",
    }, [artifact_id]


def synthetic_verification_chain(
    task_id: str,
) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    run_id = f"synthetic-{task_id}"
    generation_id = run_id
    acceptance_id = MANIFEST["tasks"][task_id]["acceptance_id"]
    spec = verifier_spec(task_id, resolved=True)
    revision_a = {"status": "known", "sha256": "sha256:" + "a" * 64}
    revision_b = {"status": "known", "sha256": "sha256:" + "b" * 64}
    events_value: list[dict[str, Any]] = []

    def append(kind: str, **fields: Any) -> None:
        sequence = len(events_value) + 1
        events_value.append(
            {
                "schema_version": 14,
                "run_id": run_id,
                "event_id": f"event-{sequence}",
                "sequence": sequence,
                "occurred_at_unix_ms": sequence,
                "event": {"kind": kind, **fields},
            }
        )

    definition = task_definition(task_id)
    definition["acceptance"][0]["verifier"] = spec
    append(
        "run_created",
        request={
            "run_id": run_id,
            "model": MODEL,
            "actor": {"kind": "root", "depth": 0},
            "task_contract": {
                "generation_id": generation_id,
                "definition": definition,
            },
            "environment": {"workspace": "/synthetic/workspace"},
        },
    )
    if task_id == "t3":
        append(
            "workspace_observed",
            workspace_state={"generation": 0, "revision": revision_a},
        )
        failure_workspace = {"generation": 1, "revision": revision_a}
        failure_outcome, failure_artifacts = synthetic_verifier_outcome(
            spec,
            "failed",
            revision_a,
        )
        append(
            "tool_prepared",
            operation_id="operation-failure",
            invocation={
                "name": "run_verifiers",
                "call_id": "call-failure",
                "arguments": {
                    "raw": canonical_bytes(
                        {"verifier_id": acceptance_id}
                    ).decode(),
                    "parsed": {"verifier_id": acceptance_id},
                },
            },
            workspace_access="may_write",
        )
        append("tool_execution_started", operation_id="operation-failure")
        append(
            "tool_outcome_committed",
            operation_id="operation-failure",
            call_id="call-failure",
            name="run_verifiers",
            outcome=failure_outcome,
            workspace_state=failure_workspace,
        )
        workspace_after_write = {"generation": 2, "revision": revision_b}
        append(
            "tool_prepared",
            operation_id="operation-write",
            invocation={"name": "edit_file", "call_id": "call-write"},
            workspace_access="may_write",
        )
        append("tool_execution_started", operation_id="operation-write")
        append(
            "tool_outcome_committed",
            operation_id="operation-write",
            call_id="call-write",
            name="edit_file",
            outcome={
                "invocation": "accepted",
                "transport": "succeeded",
                "operation": "succeeded",
                "side_effect": "applied",
                "retry": "not_needed",
                "evidence": {"status": "not_applicable", "references": []},
                "artifacts": [],
                "content": "synthetic write",
            },
            workspace_state=workspace_after_write,
        )
        lineage = {
            "policy": "failed_write_pass",
            "failure": {
                "source": {"kind": "tool", "operation_id": "operation-failure"},
                "workspace_state": failure_workspace,
                "artifact_ids": failure_artifacts,
            },
            "mutation": {
                "operation_id": "operation-write",
                "workspace_state_before": failure_workspace,
                "workspace_state_after": workspace_after_write,
            },
        }
        workspace_before = workspace_after_write
    else:
        workspace_before = {"generation": 1, "revision": revision_b}
        lineage = {"policy": "latest_pass"}
        append("workspace_observed", workspace_state=workspace_before)
    workspace_after = {
        "generation": workspace_before["generation"] + 1,
        "revision": revision_b,
    }
    candidate = {"id": "completion-final", "generation_id": generation_id, "message": "done"}
    verification_id = "host-verification:completion-final"
    receipt_id = f"receipt:{verification_id}"
    append("completion_proposed", candidate=candidate)
    append(
        "host_verification_prepared",
        verification_id=verification_id,
        candidate=candidate,
        acceptance_id=acceptance_id,
        verifier=spec,
        workspace_state_before=workspace_before,
    )
    append("host_verification_started", verification_id=verification_id)
    pass_outcome, pass_artifacts = synthetic_verifier_outcome(
        spec,
        "passed",
        revision_b,
    )
    receipt = {
        "id": receipt_id,
        "generation_id": generation_id,
        "acceptance_id": acceptance_id,
        "verification_id": verification_id,
        "verifier": spec,
        "workspace_state": workspace_after,
        "artifact_ids": pass_artifacts,
        "lineage": lineage,
    }
    append(
        "host_verification_committed",
        verification_id=verification_id,
        outcome=pass_outcome,
        receipt=receipt,
        workspace_state_after=workspace_after,
    )
    terminal = {
        "state": "completed",
        "message": "done",
        "decision": {
            "candidate_id": candidate["id"],
            "generation_id": generation_id,
            "workspace_state": workspace_after,
            "satisfied": [
                {
                    "kind": "evidence",
                    "acceptance_id": acceptance_id,
                    "receipt_id": receipt_id,
                }
            ],
        },
    }
    accounting: dict[str, Any] = {}
    terminal_outcome = {
        "run_id": run_id,
        "parent_run_id": None,
        "terminal": terminal,
        "accounting": accounting,
        "runtime_model_requests": 1,
        "runtime_retries": 0,
        "tool_calls": 1,
        "details": {"summary": "done", "evidence": [receipt]},
    }
    append("terminal", outcome=terminal_outcome)
    run = {
        "run_id": run_id,
        "model": MODEL,
        "task_contract": {
            "generation_id": generation_id,
            "definition": definition,
        },
        "workspace": "/synthetic/workspace",
        "last_sequence": len(events_value),
        "terminal": terminal,
        "accounting": accounting,
        "runtime_model_requests": 1,
        "runtime_retries": 0,
        "tool_calls": 1,
    }
    return events_value, run


class HarnessTests(unittest.TestCase):
    def test_frozen_manifest_matches_all_sources(self) -> None:
        validate_frozen_manifest()

    def test_fixture_hashes_and_initial_verifiers_are_frozen(self) -> None:
        for task_id in TASK_IDS:
            self.assertEqual(fixture_hash(task_id), MANIFEST["tasks"][task_id]["fixture_tree_sha256"])
            with tempfile.TemporaryDirectory() as raw:
                workspace = Path(raw) / "workspace"
                self.assertEqual(materialize_fixture(task_id, workspace), MANIFEST["tasks"][task_id]["fixture_base_commit"])
                result = subprocess.run(
                    ["/usr/bin/python3", "-I", "-B", "_eval_verifier.py", "."],
                    cwd=workspace,
                    env={**safe_env(), "PYTHONDONTWRITEBYTECODE": "1"},
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                    check=False,
                )
                self.assertNotEqual(result.returncode, 0)

    def test_excluded_baseline_diagnostic_identity_is_frozen(self) -> None:
        preflight_diagnostic_evidence()

    def test_schedule_is_balanced_and_complete(self) -> None:
        schedule = formal_schedule()
        self.assertEqual(len(schedule), 40)
        cells = Counter((arm["task_id"], arm["variant"]) for arm in schedule)
        self.assertTrue(all(cells[(task, variant)] == 4 for task in TASK_IDS for variant in VARIANTS))
        first = Counter(
            (arm["task_id"], arm["variant"])
            for arm in schedule
            if arm["arm_position"] == 1
        )
        self.assertTrue(all(first[(task, variant)] == 2 for task in TASK_IDS for variant in VARIANTS))

    def test_caller_and_resolved_specs_differ_only_by_host_environment(self) -> None:
        for task_id in TASK_IDS:
            caller = verifier_spec(task_id, resolved=False)
            resolved = verifier_spec(task_id, resolved=True)
            self.assertNotEqual(canonical_hash(caller), canonical_hash(resolved))
            caller["plan"]["steps"][0]["env"] = {"PYTHONDONTWRITEBYTECODE": "1"}
            self.assertEqual(caller, resolved)

    def test_canonical_verification_chain_accepts_all_frozen_policies(self) -> None:
        for task_id in TASK_IDS:
            with self.subTest(task_id=task_id):
                events_value, run = synthetic_verification_chain(task_id)
                summary = verification_summary(task_id, events_value, run)
                self.assertTrue(summary["ledger_valid"])
                self.assertTrue(summary["verification_chain_valid"])
                self.assertTrue(summary["artifact_closure_valid"])
                self.assertTrue(summary["lineage_valid"])
                self.assertEqual(summary["chain_failures"], [])
                self.assertTrue(summary["valid"])

    def test_canonical_chain_accepts_typed_rejection_after_effective_mutation(self) -> None:
        events_value, run = synthetic_verification_chain("t1")
        contract = run_created(events_value)["task_contract"]
        generation_id = contract["generation_id"]
        acceptance_id = MANIFEST["tasks"]["t1"]["acceptance_id"]
        spec = verifier_spec("t1", resolved=True)
        revision_a = {"status": "known", "sha256": "sha256:" + "a" * 64}
        revision_b = {"status": "known", "sha256": "sha256:" + "b" * 64}
        old_before = {"generation": 0, "revision": revision_a}
        old_after = {"generation": 1, "revision": revision_a}
        mutation_after = {"generation": 2, "revision": revision_b}
        final_after = {"generation": 3, "revision": revision_b}
        old_candidate = {
            "id": "completion-old",
            "generation_id": generation_id,
            "message": "too early",
        }
        old_verification = "host-verification:completion-old"
        failed_outcome, _ = synthetic_verifier_outcome(spec, "failed", revision_a)
        inserted = [
            {
                "event_id": "old-proposal",
                "event": {"kind": "completion_proposed", "candidate": old_candidate},
            },
            {
                "event_id": "old-prepared",
                "event": {
                    "kind": "host_verification_prepared",
                    "verification_id": old_verification,
                    "candidate": old_candidate,
                    "acceptance_id": acceptance_id,
                    "verifier": spec,
                    "workspace_state_before": old_before,
                },
            },
            {
                "event_id": "old-started",
                "event": {
                    "kind": "host_verification_started",
                    "verification_id": old_verification,
                },
            },
            {
                "event_id": "old-committed",
                "event": {
                    "kind": "host_verification_committed",
                    "verification_id": old_verification,
                    "outcome": failed_outcome,
                    "workspace_state_after": old_after,
                },
            },
            {
                "event_id": "old-rejected",
                "event": {
                    "kind": "completion_rejected",
                    "rejection": {
                        "candidate_id": old_candidate["id"],
                        "generation_id": generation_id,
                        "unmet_acceptance_ids": [acceptance_id],
                        "cause": "verifier_failed",
                        "required_transition": "effective_workspace_mutation",
                        "reason": "synthetic rejection",
                    },
                },
            },
            {
                "event_id": "old-mutation-prepared",
                "event": {
                    "kind": "tool_prepared",
                    "operation_id": "recovery-write",
                    "invocation": {
                        "name": "edit_file",
                        "call_id": "recovery-write",
                    },
                    "workspace_access": "may_write",
                },
            },
            {
                "event_id": "old-mutation-started",
                "event": {
                    "kind": "tool_execution_started",
                    "operation_id": "recovery-write",
                },
            },
            {
                "event_id": "old-mutation",
                "event": {
                    "kind": "tool_outcome_committed",
                    "operation_id": "recovery-write",
                    "call_id": "recovery-write",
                    "name": "edit_file",
                    "outcome": {
                        "invocation": "accepted",
                        "transport": "succeeded",
                        "operation": "succeeded",
                        "side_effect": "applied",
                        "retry": "not_needed",
                        "evidence": {"status": "not_applicable", "references": []},
                        "artifacts": [],
                        "content": "recovered",
                    },
                    "workspace_state": mutation_after,
                },
            },
        ]
        template = events_value[0]
        for stored in inserted:
            stored.update(
                schema_version=template["schema_version"],
                run_id=template["run_id"],
                occurred_at_unix_ms=1,
            )
        final_prepared = next(
            stored["event"]
            for stored in events_value
            if event_kind(stored) == "host_verification_prepared"
        )
        initial_workspace = next(
            stored["event"]
            for stored in events_value
            if event_kind(stored) == "workspace_observed"
        )
        initial_workspace["workspace_state"] = old_before
        final_prepared["workspace_state_before"] = mutation_after
        final_commit = next(
            stored["event"]
            for stored in events_value
            if event_kind(stored) == "host_verification_committed"
        )
        final_commit["workspace_state_after"].update(final_after)
        events_value[2:2] = inserted
        for sequence, stored in enumerate(events_value, 1):
            stored["sequence"] = sequence
        run["last_sequence"] = len(events_value)
        summary = verification_summary("t1", events_value, run)
        self.assertEqual(summary["completion_rejections"], 1)
        self.assertEqual(summary["host_commit_count"], 2)
        self.assertTrue(summary["valid"])
        tampered_events = copy.deepcopy(events_value)
        tampered_run = copy.deepcopy(run)
        rejection = next(
            stored["event"]["rejection"]
            for stored in tampered_events
            if event_kind(stored) == "completion_rejected"
        )
        rejection["required_transition"] = "host_verifier_execution_repair"
        self.assertFalse(
            verification_summary("t1", tampered_events, tampered_run)["valid"]
        )
        paired_tamper = copy.deepcopy(events_value)
        paired_rejection = next(
            stored["event"]["rejection"]
            for stored in paired_tamper
            if event_kind(stored) == "completion_rejected"
        )
        paired_rejection.update(
            cause="verifier_spec_mismatch",
            required_transition="host_verifier_contract_repair",
        )
        self.assertFalse(
            verification_summary("t1", paired_tamper, copy.deepcopy(run))["valid"]
        )
        legacy_events = copy.deepcopy(events_value)
        for stored in legacy_events:
            stored["schema_version"] = 13
        legacy_rejection = next(
            stored["event"]["rejection"]
            for stored in legacy_events
            if event_kind(stored) == "completion_rejected"
        )
        for field in ("generation_id", "cause", "required_transition"):
            legacy_rejection.pop(field)
        self.assertTrue(
            verification_summary("t1", legacy_events, copy.deepcopy(run))["valid"]
        )

    def test_canonical_verification_chain_rejects_independent_tampering(self) -> None:
        def locate(events_value: list[dict[str, Any]], kind: str) -> dict[str, Any]:
            return next(stored["event"] for stored in events_value if event_kind(stored) == kind)

        def tamper_terminal_message(
            events_value: list[dict[str, Any]], run: dict[str, Any]
        ) -> None:
            outcome = locate(events_value, "terminal")["outcome"]
            outcome["terminal"]["message"] = "tampered terminal"
            outcome["details"]["summary"] = "tampered terminal"
            run["terminal"]["message"] = "tampered terminal"

        cases = {
            "duplicate_event_id": lambda events_value, run: events_value[1].update(
                event_id=events_value[0]["event_id"]
            ),
            "run_terminal_parity": lambda events_value, run: run.update(
                accounting={"tampered": True}
            ),
            "root_actor": lambda events_value, run: locate(
                events_value, "run_created"
            )["request"].update(actor={"kind": "child", "depth": 0}),
            "receipt_generation": lambda events_value, run: locate(
                events_value, "host_verification_committed"
            )["receipt"].update(generation_id="old-generation"),
            "prepared_candidate": lambda events_value, run: locate(
                events_value, "host_verification_prepared"
            )["candidate"].update(id="wrong-candidate"),
            "committed_verification": lambda events_value, run: locate(
                events_value, "host_verification_committed"
            ).update(verification_id="wrong-verification"),
            "terminal_receipt": lambda events_value, run: locate(
                events_value, "terminal"
            )["outcome"]["terminal"]["decision"]["satisfied"][0].update(
                receipt_id="wrong-receipt"
            ),
            "observation_revision": lambda events_value, run: locate(
                events_value, "host_verification_committed"
            )["outcome"]["verifier_observation"].update(
                workspace_revision={"status": "known", "sha256": "sha256:" + "c" * 64}
            ),
            "artifact_inline_payload": lambda events_value, run: locate(
                events_value, "host_verification_committed"
            )["outcome"]["artifacts"][0]["inline_content"].update(
                summary="tampered"
            ),
            "latest_lineage": lambda events_value, run: locate(
                events_value, "host_verification_committed"
            )["receipt"].update(lineage={"policy": "failed_write_pass"}),
            "terminal_candidate_message": tamper_terminal_message,
            "terminal_details_summary": lambda events_value, run: locate(
                events_value, "terminal"
            )["outcome"]["details"].update(summary=""),
        }
        for name, mutate in cases.items():
            with self.subTest(case=name):
                events_value, run = synthetic_verification_chain("t1")
                mutate(events_value, run)
                self.assertFalse(verification_summary("t1", events_value, run)["valid"])

    def test_failed_write_pass_lineage_rejects_wrong_source_and_delegation(self) -> None:
        for case in ("source", "artifact", "delegated"):
            with self.subTest(case=case):
                events_value, run = synthetic_verification_chain("t3")
                commit = next(
                    stored["event"]
                    for stored in events_value
                    if event_kind(stored) == "host_verification_committed"
                )
                lineage = commit["receipt"]["lineage"]
                if case == "source":
                    lineage["failure"]["source"]["operation_id"] = "wrong-operation"
                elif case == "artifact":
                    lineage["failure"]["artifact_ids"] = ["wrong-artifact"]
                else:
                    commit["receipt"]["lineage"] = {
                        "policy": "delegated_failed_write_pass"
                    }
                summary = verification_summary("t3", events_value, run)
                self.assertFalse(summary["lineage_valid"])
                self.assertFalse(summary["valid"])

    def test_t3_lineage_binds_original_named_verifier_invocation(self) -> None:
        events_value, run = synthetic_verification_chain("t3")
        prepared = next(
            stored["event"]
            for stored in events_value
            if event_kind(stored) == "tool_prepared"
            and stored["event"].get("operation_id") == "operation-failure"
        )
        prepared["invocation"]["arguments"]["parsed"]["verifier_id"] = "wrong"
        summary = verification_summary("t3", events_value, run)
        self.assertFalse(summary["lineage_valid"])
        self.assertFalse(summary["valid"])

    def test_t3_applied_mutation_remains_effective_when_operation_reports_failure(self) -> None:
        events_value, run = synthetic_verification_chain("t3")
        mutation = next(
            stored["event"]["outcome"]
            for stored in events_value
            if event_kind(stored) == "tool_outcome_committed"
            and stored["event"].get("operation_id") == "operation-write"
        )
        mutation["operation"] = "failed"
        mutation["retry"] = "unsafe"
        summary = verification_summary("t3", events_value, run)
        self.assertTrue(summary["lineage_valid"])
        self.assertTrue(summary["valid"])

    def test_full_event_digest_and_terminal_projection_detect_payload_tampering(self) -> None:
        events_value, run = synthetic_verification_chain("t1")
        original = run_terminal_evidence(events_value, run)
        tampered_events = copy.deepcopy(events_value)
        proposal = next(
            stored["event"]["candidate"]
            for stored in tampered_events
            if event_kind(stored) == "completion_proposed"
        )
        proposal["message"] = "payload-only tamper"
        changed = run_terminal_evidence(tampered_events, run)
        self.assertNotEqual(original["events_sha256"], changed["events_sha256"])
        self.assertEqual(
            canonical_hash(
                [
                    [stored.get("sequence"), stored.get("event_id"), event_kind(stored)]
                    for stored in events_value
                ]
            ),
            canonical_hash(
                [
                    [stored.get("sequence"), stored.get("event_id"), event_kind(stored)]
                    for stored in tampered_events
                ]
            ),
        )
        tampered_run = copy.deepcopy(run)
        tampered_run["terminal"]["message"] = "run-only tamper"
        projection = run_terminal_evidence(events_value, tampered_run)
        self.assertFalse(projection["projection_equal"])
        self.assertNotEqual(
            original["run_terminal_projection_sha256"],
            projection["run_terminal_projection_sha256"],
        )

    def test_artifact_schema_rejects_rehashed_unknown_and_nul_payloads(self) -> None:
        def rebind(events_value: list[dict[str, Any]]) -> None:
            commit = next(
                stored["event"]
                for stored in events_value
                if event_kind(stored) == "host_verification_committed"
            )
            outcome = commit["outcome"]
            artifact = outcome["artifacts"][0]
            encoded = canonical_bytes(artifact["inline_content"])
            digest = sha256_bytes(encoded)
            artifact_id = f"verification-evidence:{digest}"
            artifact.update(id=artifact_id, sha256=digest, byte_len=len(encoded))
            outcome["evidence"]["references"] = [artifact_id]
            outcome["verifier_observation"]["artifact_ids"] = [artifact_id]
            commit["receipt"]["artifact_ids"] = [artifact_id]
            terminal = next(
                stored["event"]["outcome"]
                for stored in events_value
                if event_kind(stored) == "terminal"
            )
            terminal["details"]["evidence"] = [commit["receipt"]]

        for case in ("unknown", "nul"):
            with self.subTest(case=case):
                events_value, run = synthetic_verification_chain("t1")
                commit = next(
                    stored["event"]
                    for stored in events_value
                    if event_kind(stored) == "host_verification_committed"
                )
                content = commit["outcome"]["artifacts"][0]["inline_content"]
                if case == "unknown":
                    content["unexpected"] = True
                else:
                    content["summary"] = "bad\0summary"
                rebind(events_value)
                self.assertFalse(
                    verification_summary("t1", events_value, run)["valid"]
                )

    def test_porcelain_parser_preserves_first_path_character(self) -> None:
        self.assertEqual(
            parse_changed_files(
                " M slugify.py\n?? new.py\nR  old.py -> renamed.py\n",
            ),
            ["new.py", "renamed.py", "slugify.py"],
        )

    def test_child_lifecycle_closes_parent_start_result_finish_and_terminal(self) -> None:
        root_id = "root-1"
        child_id = "child-1"
        task_contract = {"generation_id": child_id, "definition": {}}
        task = {
            "task_id": "task-1",
            "root_run_id": root_id,
            "parent_run_id": root_id,
            "child_run_id": child_id,
            "call_id": "call-1",
            "task_contract": task_contract,
            "workspace": {
                "access": "read_only",
                "root_workspace": "/workspace",
            },
            "tool_policy": {"enabled": True},
            "limits": {"max_turns": 1},
        }
        outcome = {
            "run_id": child_id,
            "parent_run_id": root_id,
            "terminal": {"state": "completed", "message": "done"},
            "accounting": {},
            "runtime_model_requests": 1,
            "runtime_retries": 0,
            "tool_calls": 0,
            "details": {"summary": "done"},
        }
        handoff_payload = json.dumps(
            {
                "run_id": child_id,
                "terminal": outcome["terminal"],
                "summary": "done",
            },
            ensure_ascii=False,
            separators=(",", ":"),
        )
        handoff = (
            f'<codewhale:runtime_event kind="subagent_completion" '
            f'agent_id="{child_id}">\n{handoff_payload}\n</codewhale:runtime_event>'
        )
        root_events = [
            {
                "run_id": root_id,
                "sequence": 1,
                "event": {
                    "kind": "run_created",
                    "request": {"run_id": root_id, "actor": {"kind": "root", "depth": 0}},
                },
            },
            {
                "run_id": root_id,
                "sequence": 2,
                "event": {"kind": "agent_task_prepared", "task": task},
            },
            {
                "run_id": root_id,
                "sequence": 3,
                "event": {
                    "kind": "child_started",
                    "task_id": "task-1",
                    "call_id": "call-1",
                    "child_run_id": child_id,
                    "depth": 1,
                },
            },
            {
                "run_id": root_id,
                "sequence": 4,
                "event": {
                    "kind": "agent_result_collected",
                    "task_id": "task-1",
                    "outcome": outcome,
                },
            },
            {
                "run_id": root_id,
                "sequence": 5,
                "event": {
                    "kind": "child_finished",
                    "call_id": "call-1",
                    "outcome": outcome,
                    "accounting": {"root": "cumulative"},
                    "handoff_content": handoff,
                },
            },
            {
                "run_id": root_id,
                "sequence": 6,
                "event": {
                    "kind": "model_request_prepared",
                    "request": {
                        "messages": [{"role": "user", "content": handoff}]
                    },
                },
            },
        ]
        child_events = [
            {
                "run_id": child_id,
                "sequence": 1,
                "event": {
                    "kind": "run_created",
                    "request": {
                        "run_id": child_id,
                        "parent_run_id": root_id,
                        "task_contract": task_contract,
                        "actor": {"kind": "child", "depth": 1},
                        "agent_task": task,
                        "tool_policy": task["tool_policy"],
                        "limits": task["limits"],
                        "environment": {"workspace": "/workspace"},
                    },
                },
            },
            {
                "run_id": child_id,
                "sequence": 2,
                "event": {"kind": "terminal", "outcome": outcome},
            },
        ]
        child = {"run_id": child_id, "parent_run_id": root_id}
        self.assertTrue(
            child_lifecycle_evidence(root_events, 2, task, child_events, child)[
                "valid"
            ]
        )
        for case in ("task_parent", "child_parent", "start", "result", "finish"):
            with self.subTest(case=case):
                tampered_root = copy.deepcopy(root_events)
                tampered_task = tampered_root[1]["event"]["task"]
                tampered_child = copy.deepcopy(child)
                if case == "task_parent":
                    tampered_task["parent_run_id"] = "wrong"
                elif case == "child_parent":
                    tampered_child["parent_run_id"] = "wrong"
                elif case == "start":
                    tampered_root[2]["event"]["child_run_id"] = "wrong"
                elif case == "result":
                    tampered_root[3]["event"]["outcome"] = {"run_id": "wrong"}
                else:
                    tampered_root[4]["event"]["outcome"] = {"run_id": "wrong"}
                evidence = child_lifecycle_evidence(
                    tampered_root,
                    2,
                    tampered_task,
                    child_events,
                    tampered_child,
                )
                self.assertFalse(evidence["valid"])

    def test_candidate_task_contract_mismatch_is_a_safety_reject(self) -> None:
        arms = synthetic_formal_arms(lambda _: False)
        candidate = next(arm for arm in arms if arm["variant"] == "candidate")
        candidate["task_definition_sha256"] = "sha256:" + "0" * 64
        candidate["measurement_valid"] = False
        self.assertIn("arm_identity_mismatch", arm_measurement_failures(candidate))
        self.assertIn(
            "candidate_task_contract_mismatch",
            arm_safety_failures(candidate),
        )
        self.assertEqual(summarize(arms)["decision"], "reject")

    def test_summary_requires_three_more_verified_arms(self) -> None:
        arms = synthetic_formal_arms(
            lambda arm: arm["variant"] == "candidate"
            and arm["task_id"] == "t1"
            and arm["run_index"] <= 3,
        )
        result = summarize(arms)
        self.assertEqual(result["verified_success_delta"], 3)
        self.assertEqual(result["decision"], "keep")

    def test_equal_success_keeps_only_with_preregistered_paired_efficiency(self) -> None:
        arms = synthetic_formal_arms(
            lambda _: True,
            lambda arm: {
                "tokens": 8_000 if arm["variant"] == "candidate" else 10_000,
            },
        )
        result = summarize(arms)
        self.assertEqual(result["verified_success_delta"], 0)
        self.assertEqual(
            result["paired_efficiency"]["median_improvement_basis_points"]["tokens"],
            2_000,
        )
        self.assertEqual(result["decision"], "keep")

        exact = synthetic_formal_arms(
            lambda _: True,
            lambda arm: {
                "tokens": 8_500 if arm["variant"] == "candidate" else 10_000,
            },
        )
        self.assertEqual(summarize(exact)["decision"], "keep")

        below = synthetic_formal_arms(
            lambda _: True,
            lambda arm: {
                "tokens": 8_501 if arm["variant"] == "candidate" else 10_000,
            },
        )
        self.assertEqual(summarize(below)["decision"], "reject")

    def test_efficiency_rejects_other_metric_regression_over_ten_percent(self) -> None:
        arms = synthetic_formal_arms(
            lambda _: True,
            lambda arm: {
                "tokens": 8_000 if arm["variant"] == "candidate" else 10_000,
                "cost_nanousd": 11_001_000
                if arm["variant"] == "candidate"
                else 10_000_000,
            },
        )
        self.assertEqual(summarize(arms)["decision"], "reject")

        exact_boundary = synthetic_formal_arms(
            lambda _: True,
            lambda arm: {
                "tokens": 8_000 if arm["variant"] == "candidate" else 10_000,
                "cost_nanousd": 11_000_000
                if arm["variant"] == "candidate"
                else 10_000_000,
            },
        )
        self.assertEqual(summarize(exact_boundary)["decision"], "keep")

    def test_paired_efficiency_requires_fifteen_pairs_and_three_per_task(self) -> None:
        metrics = lambda arm: {
            "tokens": 8_000 if arm["variant"] == "candidate" else 10_000,
        }
        exact = synthetic_formal_arms(
            lambda arm: arm["run_index"] <= 3,
            metrics,
        )
        exact_summary = summarize(exact)
        self.assertEqual(exact_summary["paired_efficiency"]["dual_success_pairs"], 15)
        self.assertTrue(exact_summary["paired_efficiency"]["sufficient"])
        self.assertEqual(exact_summary["decision"], "keep")

        fourteen = synthetic_formal_arms(
            lambda arm: arm["run_index"] <= 3
            and not (arm["task_id"] == "t5" and arm["run_index"] == 3),
            metrics,
        )
        self.assertEqual(summarize(fourteen)["paired_efficiency"]["dual_success_pairs"], 14)
        self.assertEqual(summarize(fourteen)["decision"], "reject")

        one_task_short = synthetic_formal_arms(
            lambda arm: arm["task_id"] != "t5" or arm["run_index"] <= 2,
            metrics,
        )
        one_task_summary = summarize(one_task_short)
        self.assertGreaterEqual(one_task_summary["paired_efficiency"]["dual_success_pairs"], 15)
        self.assertEqual(one_task_summary["paired_efficiency"]["pairs_per_task"]["t5"], 2)
        self.assertFalse(one_task_summary["paired_efficiency"]["sufficient"])
        self.assertEqual(one_task_summary["decision"], "reject")

    def test_incomplete_or_duplicated_schedule_holds(self) -> None:
        arms = synthetic_formal_arms(lambda _: False)
        self.assertEqual(summarize(arms[:-1])["decision"], "hold")
        duplicated = [*arms[:-1], copy.deepcopy(arms[0])]
        self.assertEqual(summarize(duplicated)["decision"], "hold")
        out_of_order = copy.deepcopy(arms)
        out_of_order[0], out_of_order[1] = out_of_order[1], out_of_order[0]
        self.assertEqual(summarize(out_of_order)["decision"], "hold")

    def test_per_task_success_regression_rejects_even_with_aggregate_gain(self) -> None:
        arms = synthetic_formal_arms(
            lambda arm: (
                arm["variant"] == "baseline" and arm["task_id"] == "t1"
            )
            or (
                arm["variant"] == "candidate"
                and (
                    (arm["task_id"] == "t1" and arm["run_index"] <= 3)
                    or arm["task_id"] == "t2"
                )
            ),
        )
        result = summarize(arms)
        self.assertEqual(result["verified_success_delta"], 3)
        self.assertFalse(result["hard_gates"]["per_task_success_non_regression"])
        self.assertEqual(result["decision"], "reject")

    def test_safety_failure_rejects_while_measurement_failure_holds(self) -> None:
        arms = synthetic_formal_arms(lambda _: False)
        arms[0]["false_success"] = True
        arms[0]["terminal"]["state"] = "completed"
        self.assertEqual(summarize(arms)["decision"], "reject")

        measured = synthetic_formal_arms(lambda _: False)
        measured[0]["state_schema"]["valid"] = False
        measured[0]["measurement_valid"] = False
        self.assertFalse(measured[0]["false_success"])
        self.assertEqual(summarize(measured)["decision"], "hold")

        evidenced_failure = synthetic_formal_arms(lambda _: False)
        evidenced_failure[0]["terminal"]["state"] = "completed"
        evidenced_failure[0]["measurement_valid"] = False
        evidenced_failure[0]["state_schema"]["valid"] = False
        evidenced_failure[0]["false_success"] = is_false_success(
            "completed",
            False,
            evidenced_failure[0]["behavior_evidence_known"],
        )
        self.assertTrue(evidenced_failure[0]["false_success"])
        self.assertEqual(summarize(evidenced_failure)["decision"], "reject")

        self.assertFalse(is_false_success("completed", True, False))
        self.assertFalse(is_false_success("completed", False, False))
        self.assertTrue(is_false_success("completed", False, True))

        otherwise_keep = synthetic_formal_arms(
            lambda arm: arm["variant"] == "candidate"
            and arm["task_id"] == "t1"
            and arm["run_index"] <= 3,
        )
        self.assertEqual(summarize(otherwise_keep)["decision"], "keep")
        self.assertEqual(
            summarize(otherwise_keep, {"code": "external_verifier_incomplete"})[
                "decision"
            ],
            "hold",
        )
        otherwise_keep[0]["false_success"] = True
        otherwise_keep[0]["terminal"]["state"] = "completed"
        self.assertEqual(
            summarize(otherwise_keep, {"code": "external_verifier_incomplete"})[
                "decision"
            ],
            "reject",
        )

    def test_per_arm_cost_boundary_is_exact(self) -> None:
        arm = synthetic_formal_arms(lambda _: False)[0]
        arm["accounting"]["cost_nanousd"] = RESOURCES["max_known_cost_nanousd_per_arm"]
        self.assertNotIn("per_arm_known_cost_limit", arm_measurement_failures(arm))
        arm["accounting"]["cost_nanousd"] += 1
        self.assertIn("per_arm_known_cost_limit", arm_measurement_failures(arm))
        self.assertEqual(suite_cost_limit(True), 800_000_000)
        self.assertEqual(suite_cost_limit(False), 120_000_000)
        self.assertTrue(has_cost_headroom(780_000_000, 800_000_000))
        self.assertFalse(has_cost_headroom(780_000_001, 800_000_000))

    def test_physical_attempts_are_not_double_counted_with_transport_retries(self) -> None:
        run = {
            "accounting": {
                "hard_request_limit": RESOURCES["max_physical_api_attempts_per_arm"],
                "root": {"started": 2, "completed": 2, "in_flight": 0, "retries": 1},
                "child": {"started": 1, "completed": 1, "in_flight": 0, "retries": 0},
                "transport_retries": 1,
                "billing_unknown_attempts": 0,
                "usage_responses": 2,
                "usage_missing_responses": 0,
                "incomplete_responses": 0,
                "unpriced_usage_responses": 0,
                "records_after_seal": 0,
                "cost_nanousd": 1,
                "cost_nanocny": 1,
                "complete": True,
                "usage_complete": True,
                "billing_unknown": False,
                "unpriced": False,
                "sealed": True,
                "surface_usage": [
                    {
                        "surface": "standard_chat",
                        "model": MODEL,
                        "response_count": 2,
                        "usage_response_count": 2,
                        "usage": {},
                        "cost_nanousd": 1,
                        "cost_nanocny": 1,
                    }
                ],
            },
            "usage": {},
            "runtime_retries": 0,
        }
        summary = usage_summary(run)
        self.assertEqual(summary["requests"]["started"], 3)
        self.assertEqual(summary["requests"]["physical_attempts"], 3)
        self.assertEqual(summary["transport_retries"], 1)
        self.assertTrue(summary["valid"])
        self.assertTrue(summary["retry_attribution_valid"])
        self.assertFalse(summary["retry_treatment_valid"])
        self.assertFalse(summary["execution_identity_valid"])
        self.assertFalse(accounting_cost_is_lower_bound(summary))

        run["accounting"]["transport_retries"] = 0
        run["accounting"]["root"]["retries"] = 0
        self.assertTrue(usage_summary(run)["execution_identity_valid"])
        run["accounting"]["hard_request_limit"] = 9
        self.assertFalse(usage_summary(run)["budget_identity_valid"])
        run["accounting"]["hard_request_limit"] = None
        self.assertFalse(usage_summary(run)["budget_identity_valid"])
        run["accounting"]["hard_request_limit"] = RESOURCES[
            "max_physical_api_attempts_per_arm"
        ]
        run["accounting"]["surface_usage"][0]["surface"] = "strict_chat"
        strict = usage_summary(run)
        self.assertTrue(strict["valid"])
        self.assertTrue(strict["surface_identity_mismatch"])
        self.assertFalse(strict["execution_identity_valid"])
        run["accounting"]["surface_usage"][0]["surface"] = "standard_chat"
        run["accounting"]["root"]["retries"] = 1
        retries = usage_summary(run)
        self.assertFalse(retries["retry_attribution_valid"])
        self.assertFalse(retries["valid"])

    def test_tree_logical_request_limit_includes_child_requests(self) -> None:
        arm = synthetic_formal_arms(lambda _: False)[0]
        arm["event_counts"]["model_request_prepared"] = 8
        arm["tree_event_counts"]["model_request_prepared"] = 11
        self.assertIn("logical_request_limit_exceeded", arm_measurement_failures(arm))
        arm["tree_event_counts"]["model_request_prepared"] = 8
        arm["event_counts"]["tool_prepared"] = 20
        arm["tree_event_counts"]["tool_prepared"] = 25
        self.assertIn("tool_call_limit_exceeded", arm_measurement_failures(arm))

    def test_execution_identity_is_hold_for_baseline_and_reject_for_candidate(self) -> None:
        unobserved = synthetic_formal_arms(lambda _: False)
        unobserved_selected = next(
            arm for arm in unobserved if arm["variant"] == "candidate"
        )
        unobserved_selected["accounting"].update(
            surface_usage=[],
            surface_totals_valid=True,
            surface_identity_observed=False,
            surface_identity_mismatch=False,
            surface_identity_valid=False,
            surface_valid=False,
            execution_identity_valid=False,
        )
        unobserved_selected["measurement_valid"] = False
        self.assertEqual(summarize(unobserved)["decision"], "hold")

        candidate = synthetic_formal_arms(lambda _: False)
        selected = next(arm for arm in candidate if arm["variant"] == "candidate")
        selected["accounting"]["execution_identity_valid"] = False
        selected["accounting"]["surface_identity_valid"] = False
        selected["accounting"]["surface_identity_mismatch"] = True
        selected["measurement_valid"] = False
        result = summarize(candidate)
        self.assertIn(
            "candidate_execution_identity_mismatch",
            next(iter(result["safety_failures"].values())),
        )
        self.assertEqual(result["decision"], "reject")

        retry_baseline = synthetic_formal_arms(lambda _: False)
        retry_baseline_selected = next(
            arm for arm in retry_baseline if arm["variant"] == "baseline"
        )
        retry_baseline_selected["accounting"].update(
            transport_retries=1,
            retry_attribution_valid=True,
            retry_treatment_valid=False,
            execution_identity_valid=False,
        )
        retry_baseline_selected["measurement_valid"] = False
        self.assertIn(
            "transport_retry_limit_exceeded",
            arm_measurement_failures(retry_baseline_selected),
        )
        self.assertEqual(summarize(retry_baseline)["decision"], "hold")

        retry_candidate = synthetic_formal_arms(lambda _: False)
        retry_selected = next(
            arm for arm in retry_candidate if arm["variant"] == "candidate"
        )
        retry_selected["accounting"].update(
            transport_retries=1,
            retry_attribution_valid=True,
            retry_treatment_valid=False,
            execution_identity_valid=False,
        )
        retry_selected["measurement_valid"] = False
        retry_result = summarize(retry_candidate)
        self.assertIn(
            "candidate_execution_identity_mismatch",
            next(iter(retry_result["safety_failures"].values())),
        )
        self.assertEqual(retry_result["decision"], "reject")

        attribution_candidate = synthetic_formal_arms(lambda _: False)
        attribution_selected = next(
            arm for arm in attribution_candidate if arm["variant"] == "candidate"
        )
        attribution_selected["accounting"].update(
            retry_attribution_valid=False,
            retry_treatment_valid=False,
            execution_identity_valid=False,
            valid=False,
        )
        attribution_selected["measurement_valid"] = False
        attribution_result = summarize(attribution_candidate)
        self.assertIn(
            "candidate_accounting_identity_mismatch",
            next(iter(attribution_result["safety_failures"].values())),
        )
        self.assertEqual(attribution_result["decision"], "reject")

    def test_output_claim_is_private_and_never_clobbers(self) -> None:
        output = ROOT / "eval/results" / f".m7-self-test-{uuid.uuid4().hex}.json"
        partial = output.with_suffix(output.suffix + ".partial")
        try:
            self.assertEqual(validate_fresh_output(output), output.resolve(strict=False))
            claim_private_output(output, {"status": "claimed"})
            self.assertEqual(stat.S_IMODE(output.stat().st_mode), 0o600)
            with self.assertRaises(EvaluationError):
                validate_fresh_output(output)
            sentinel = output.read_bytes()
            with self.assertRaises(EvaluationError):
                claim_private_output(output, {"status": "replacement"})
            self.assertEqual(output.read_bytes(), sentinel)
        finally:
            output.unlink(missing_ok=True)
            partial.unlink(missing_ok=True)

    def test_formal_output_identity_cannot_be_changed_to_resample(self) -> None:
        expected = (
            ROOT
            / "eval/results/m7-a-agent-convergence-formal-3351213b-vs-24c8a530.json"
        ).resolve(strict=False)
        alternate = (
            ROOT / "eval/results" / f".m7-self-test-resample-{uuid.uuid4().hex}.json"
        )
        self.assertEqual(expected_output_path(formal=True), expected)
        with self.assertRaisesRegex(
            EvaluationError,
            "frozen_output_identity_mismatch",
        ):
            validate_suite_output(alternate, formal=True)

    def test_output_preflight_rejects_outside_and_symlink_without_clobber(self) -> None:
        link = ROOT / "eval/results" / f".m7-self-test-link-{uuid.uuid4().hex}.json"
        with tempfile.TemporaryDirectory() as raw:
            sentinel = Path(raw) / "sentinel"
            sentinel.write_bytes(b"keep")
            link.symlink_to(sentinel)
            try:
                with self.assertRaises(EvaluationError):
                    validate_fresh_output(link)
                with self.assertRaises(EvaluationError):
                    validate_fresh_output(Path(raw) / "outside.json")
                self.assertEqual(sentinel.read_bytes(), b"keep")
            finally:
                link.unlink(missing_ok=True)

    def test_binary_preflight_failure_reads_no_key_and_executes_no_arm(self) -> None:
        output = ROOT / "eval/results" / f".m7-self-test-preflight-{uuid.uuid4().hex}.json"
        partial = output.with_suffix(output.suffix + ".partial")
        with tempfile.TemporaryDirectory() as raw:
            binary = Path(raw) / "codewhale"
            binary.write_bytes(b"not-the-frozen-binary")
            binary.chmod(0o500)
            args = argparse.Namespace(
                acknowledge_cost=True,
                key_file=Path(raw) / "key",
                baseline_binary=binary,
                baseline_revision=MANIFEST["binary_identities"]["baseline"]["revision"],
                candidate_binary=binary,
                candidate_revision=MANIFEST["binary_identities"]["candidate"]["revision"],
                output=output,
            )
            with (
                mock.patch.object(
                    sys.modules[__name__],
                    "expected_output_path",
                    return_value=output.resolve(strict=False),
                ),
                mock.patch.object(sys.modules[__name__], "preflight_fixtures"),
                mock.patch.object(sys.modules[__name__], "preflight_diagnostic_evidence"),
                mock.patch.object(CANARY, "read_key") as read_key,
                mock.patch.object(sys.modules[__name__], "execute_arm") as execute,
            ):
                with self.assertRaises(EvaluationError):
                    run_suite(args, formal=True)
                read_key.assert_not_called()
                execute.assert_not_called()
            self.assertFalse(output.exists())
        output.unlink(missing_ok=True)
        partial.unlink(missing_ok=True)

    def test_run_suite_checkpoints_then_stops_on_safety_or_unknown_billing(self) -> None:
        first = formal_schedule()[0]
        for case in ("safety", "billing", "state"):
            output = ROOT / "eval/results" / f".m7-self-test-{case}-{uuid.uuid4().hex}.json"
            partial = output.with_suffix(output.suffix + ".partial")
            with tempfile.TemporaryDirectory() as raw:
                baseline = Path(raw) / "baseline"
                candidate = Path(raw) / "candidate"
                baseline.write_bytes(b"baseline")
                candidate.write_bytes(b"candidate")
                baseline.chmod(0o500)
                candidate.chmod(0o500)
                args = argparse.Namespace(
                    acknowledge_cost=True,
                    key_file=Path(raw) / "key",
                    baseline_binary=baseline,
                    baseline_revision=MANIFEST["binary_identities"]["baseline"]["revision"],
                    candidate_binary=candidate,
                    candidate_revision=MANIFEST["binary_identities"]["candidate"]["revision"],
                    output=output,
                )
                arm = synthetic_arm(
                    first,
                    verified=False,
                    false_success=case == "safety",
                )
                if case == "billing":
                    arm["measurement_valid"] = False
                    arm["accounting"]["valid"] = False
                    arm["accounting"]["billing_unknown"] = True
                    arm["accounting"]["billing_unknown_attempts"] = 1
                if case == "state":
                    arm["measurement_valid"] = False
                    arm["state_schema"]["valid"] = False
                with (
                    mock.patch.object(
                        sys.modules[__name__],
                        "expected_output_path",
                        return_value=output.resolve(strict=False),
                    ),
                    mock.patch.object(sys.modules[__name__], "preflight_fixtures"),
                    mock.patch.object(sys.modules[__name__], "preflight_diagnostic_evidence"),
                    mock.patch.object(
                        sys.modules[__name__],
                        "preflight_binary",
                        return_value={"identity": "frozen"},
                    ),
                    mock.patch.object(CANARY, "read_key", return_value="test-secret"),
                    mock.patch.object(
                        sys.modules[__name__],
                        "execute_arm",
                        return_value=arm,
                    ) as execute,
                ):
                    result = run_suite(args, formal=True)
                self.assertEqual(execute.call_count, 1)
                self.assertEqual(len(result["arms"]), 1)
                self.assertIsNone(result["active_arm"])
                self.assertEqual(
                    result["abort"]["code"],
                    {
                        "safety": "safety_gate_failed",
                        "billing": "aborted_unknown_billing",
                        "state": "measurement_invalid",
                    }[case],
                )
                self.assertEqual(
                    result["aggregate"]["decision"],
                    "reject" if case == "safety" else "hold",
                )
                self.assertEqual(stat.S_IMODE(output.stat().st_mode), 0o600)
                if case == "billing":
                    self.assertTrue(result["known_cost_is_lower_bound"])
                if case == "state":
                    self.assertFalse(result["known_cost_is_lower_bound"])
            output.unlink(missing_ok=True)
            partial.unlink(missing_ok=True)

    def test_key_failure_is_durable_before_api_and_contains_no_secret(self) -> None:
        output = ROOT / "eval/results" / f".m7-self-test-key-{uuid.uuid4().hex}.json"
        partial = output.with_suffix(output.suffix + ".partial")
        with tempfile.TemporaryDirectory() as raw:
            baseline = Path(raw) / "baseline"
            candidate = Path(raw) / "candidate"
            baseline.write_bytes(b"baseline")
            candidate.write_bytes(b"candidate")
            baseline.chmod(0o500)
            candidate.chmod(0o500)
            args = argparse.Namespace(
                acknowledge_cost=True,
                key_file=Path(raw) / "missing-key",
                baseline_binary=baseline,
                baseline_revision=MANIFEST["binary_identities"]["baseline"]["revision"],
                candidate_binary=candidate,
                candidate_revision=MANIFEST["binary_identities"]["candidate"]["revision"],
                output=output,
            )
            with (
                mock.patch.object(
                    sys.modules[__name__],
                    "expected_output_path",
                    return_value=output.resolve(strict=False),
                ),
                mock.patch.object(sys.modules[__name__], "preflight_fixtures"),
                mock.patch.object(sys.modules[__name__], "preflight_diagnostic_evidence"),
                mock.patch.object(
                    sys.modules[__name__],
                    "preflight_binary",
                    return_value={"identity": "frozen"},
                ),
                mock.patch.object(
                    CANARY,
                    "read_key",
                    side_effect=CANARY.Failure("key_unavailable"),
                ),
                mock.patch.object(sys.modules[__name__], "execute_arm") as execute,
            ):
                result = run_suite(args, formal=True)
            execute.assert_not_called()
            self.assertEqual(result["execution_state"], "aborted_before_api")
            self.assertEqual(result["abort"]["code"], "key_unavailable")
            self.assertFalse(result["product_metric_eligible"])
            self.assertEqual(stat.S_IMODE(output.stat().st_mode), 0o600)
            persisted = json.loads(output.read_text(encoding="utf-8"))
            self.assertEqual(persisted["abort"]["code"], "key_unavailable")
            self.assertIsNone(persisted["active_arm"])
        output.unlink(missing_ok=True)
        partial.unlink(missing_ok=True)

    def test_post_reservation_exception_is_preserved_as_lower_bound(self) -> None:
        output = ROOT / "eval/results" / f".m7-self-test-exception-{uuid.uuid4().hex}.json"
        partial = output.with_suffix(output.suffix + ".partial")
        with tempfile.TemporaryDirectory() as raw:
            baseline = Path(raw) / "baseline"
            candidate = Path(raw) / "candidate"
            baseline.write_bytes(b"baseline")
            candidate.write_bytes(b"candidate")
            baseline.chmod(0o500)
            candidate.chmod(0o500)
            args = argparse.Namespace(
                acknowledge_cost=True,
                key_file=Path(raw) / "key",
                baseline_binary=baseline,
                baseline_revision=MANIFEST["binary_identities"]["baseline"]["revision"],
                candidate_binary=candidate,
                candidate_revision=MANIFEST["binary_identities"]["candidate"]["revision"],
                output=output,
            )

            def fail_after_reservation(*_args: Any, **_kwargs: Any) -> dict[str, Any]:
                checkpoint = json.loads(output.read_text(encoding="utf-8"))
                self.assertEqual(
                    checkpoint["active_arm"]["status"],
                    "reserved_before_api",
                )
                self.assertTrue(checkpoint["known_cost_is_lower_bound"])
                raise RuntimeError("redacted")

            with (
                mock.patch.object(
                    sys.modules[__name__],
                    "expected_output_path",
                    return_value=output.resolve(strict=False),
                ),
                mock.patch.object(sys.modules[__name__], "preflight_fixtures"),
                mock.patch.object(sys.modules[__name__], "preflight_diagnostic_evidence"),
                mock.patch.object(
                    sys.modules[__name__],
                    "preflight_binary",
                    return_value={"identity": "frozen"},
                ),
                mock.patch.object(CANARY, "read_key", return_value="test-secret"),
                mock.patch.object(
                    sys.modules[__name__],
                    "execute_arm",
                    side_effect=fail_after_reservation,
                ) as execute,
            ):
                result = run_suite(args, formal=True)
            self.assertEqual(execute.call_count, 1)
            self.assertEqual(result["abort"]["code"], "unexpected_execution_failure")
            self.assertEqual(result["active_arm"]["status"], "failed_after_reservation")
            self.assertTrue(result["known_cost_is_lower_bound"])
            self.assertEqual(result["aggregate"]["decision"], "hold")
            self.assertNotIn("redacted", output.read_text(encoding="utf-8"))
        output.unlink(missing_ok=True)
        partial.unlink(missing_ok=True)


def freeze_report() -> dict[str, Any]:
    schedule = formal_schedule()
    return {
        "manifest_content_sha256": manifest_content_hash(),
        "harness_sha256": file_hash(Path(__file__)),
        "canary_helper_sha256": file_hash(CANARY_PATH),
        "schedule_sha256": canonical_hash(schedule),
        "binary_identities_sha256": canonical_hash(MANIFEST["binary_identities"]),
        "build_inputs_sha256": {
            "cargo_lock": file_hash(ROOT / "Cargo.lock"),
            "rust_toolchain": file_hash(ROOT / "rust-toolchain.toml"),
        },
        "fixture_tree_sha256": {task: fixture_hash(task) for task in TASK_IDS},
        "task_definition_sha256": {task: canonical_hash(task_definition(task)) for task in TASK_IDS},
        "runtime_task_definition_sha256": {
            variant: {
                task: canonical_hash(runtime_task_definition(task, variant))
                for task in TASK_IDS
            }
            for variant in VARIANTS
        },
        "caller_verifier_spec_sha256": {task: canonical_hash(verifier_spec(task, resolved=False)) for task in TASK_IDS},
        "resolved_verifier_spec_sha256": {task: canonical_hash(verifier_spec(task, resolved=True)) for task in TASK_IDS},
        "formal_arms": len(schedule),
    }


def validate_frozen_manifest() -> None:
    require(
        MANIFEST.get("status") == FROZEN_STATUS,
        "manifest_not_frozen",
    )
    require(
        MANIFEST.get("protocol_schemas")
        == {
            "baseline": {"run_api": 9, "runtime_event": 13, "state": 18},
            "candidate": {"run_api": 9, "runtime_event": 14, "state": 19},
        }
        and MANIFEST.get("acceptance", {}).get("shrink_enabled") is False,
        "manifest_protocol_or_decision_invalid",
    )
    expected = MANIFEST.get("frozen_hashes")
    require(isinstance(expected, dict), "frozen_hashes_missing")
    actual = freeze_report()
    require(
        actual["build_inputs_sha256"]
        == {
            "cargo_lock": MANIFEST["build_identity"]["cargo_lock_sha256"],
            "rust_toolchain": MANIFEST["build_identity"]["rust_toolchain_sha256"],
        },
        "build_input_identity_mismatch",
    )
    comparisons = {
        "manifest_content_sha256_excluding_frozen_hashes": actual["manifest_content_sha256"],
        "harness_sha256": actual["harness_sha256"],
        "canary_helper_sha256": actual["canary_helper_sha256"],
        "schedule_sha256": actual["schedule_sha256"],
        "binary_identities_sha256": actual["binary_identities_sha256"],
        "build_inputs_sha256": actual["build_inputs_sha256"],
        "fixture_tree_sha256": actual["fixture_tree_sha256"],
        "task_definition_sha256": actual["task_definition_sha256"],
        "runtime_task_definition_sha256": actual[
            "runtime_task_definition_sha256"
        ],
        "caller_verifier_spec_sha256": actual["caller_verifier_spec_sha256"],
        "resolved_verifier_spec_sha256": actual["resolved_verifier_spec_sha256"],
    }
    require(
        all(expected.get(name) == value for name, value in comparisons.items()),
        "frozen_hash_mismatch",
        {
            "mismatched": sorted(
                name for name, value in comparisons.items() if expected.get(name) != value
            )
        },
    )


def parser() -> argparse.ArgumentParser:
    value = argparse.ArgumentParser(description=__doc__)
    sub = value.add_subparsers(dest="command", required=True)
    sub.add_parser("self-test")
    sub.add_parser("freeze-report")
    for name in ("diagnostic", "formal"):
        command = sub.add_parser(name)
        command.add_argument("--baseline-binary", type=Path, required=True)
        command.add_argument("--baseline-revision", required=True)
        command.add_argument("--candidate-binary", type=Path)
        command.add_argument("--candidate-revision")
        command.add_argument("--key-file", type=Path, required=True)
        command.add_argument("--output", type=Path, required=True)
        command.add_argument("--acknowledge-cost", action="store_true")
    return value


def main() -> int:
    args = parser().parse_args()
    if args.command == "self-test":
        suite = unittest.defaultTestLoader.loadTestsFromTestCase(HarnessTests)
        return 0 if unittest.TextTestRunner(verbosity=2).run(suite).wasSuccessful() else 1
    if args.command == "freeze-report":
        print(json.dumps(freeze_report(), ensure_ascii=False, sort_keys=True, separators=(",", ":")))
        return 0
    result = run_suite(args, formal=args.command == "formal")
    print(
        json.dumps(
            {
                "output": str(args.output),
                "mode": result["mode"],
                "arms": len(result["arms"]),
                "abort": result["abort"],
                "product_metric_eligible": result["product_metric_eligible"],
                "decision": result.get("aggregate", {}).get("decision") if result.get("aggregate") else None,
            },
            ensure_ascii=False,
            sort_keys=True,
            separators=(",", ":"),
        )
    )
    return 0 if result["abort"] is None else 2


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (EvaluationError, CANARY.Failure) as error:
        print(f"evaluation_error:{error.code}", file=sys.stderr)
        raise SystemExit(2)
