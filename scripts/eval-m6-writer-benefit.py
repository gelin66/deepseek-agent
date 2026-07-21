#!/usr/bin/env python3
"""Formal same-binary DeepSeek A/B for the M6 isolated Writer treatment.

The evaluator talks only to the canonical ``app-server --stdio`` Run API.
It stores measurements and typed lifecycle facts, never prompts, model text,
reasoning, tool arguments, tool output, stderr, credentials, or temporary paths.
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
from pathlib import Path
from typing import Any
from unittest import mock


ROOT = Path(__file__).resolve().parents[1]
MANIFEST_PATH = ROOT / "eval/manifests/m6-b1-writer-benefit-ab-v3.json"
CANARY_PATH = ROOT / "scripts/eval-m6-writer-canary.py"
RESULT_SCHEMA = "codewhale.eval.m6-writer-benefit.v3"
RUN_API_SCHEMA = 9
RUNTIME_EVENT_SCHEMA = 13
STATE_SCHEMA = 18
MODEL = "deepseek-v4-flash"
TREATMENTS = ("single", "writer")
TASK_IDS = ("t1", "t2", "t3")
RUNS_PER_CELL = 6
MAX_PAIR_ATTEMPTS = 1
PYTHON = Path("/usr/bin/python3")
SECRET_BOUNDARY_FAILURE_CODES = {
    "key_in_fixture",
    "key_in_protocol",
    "key_in_result_file",
    "result_contains_key",
    "secret_scan_incomplete",
    "key_in_state",
    "key_in_stderr",
}
WRITER_CHILD_TOOLS = [
    "apply_patch",
    "edit_file",
    "git_diff",
    "git_status",
    "grep_files",
    "list_dir",
    "read_file",
    "run_verifiers",
]
LIFECYCLE_KINDS = (
    "agent_task_prepared",
    "agent_workspace_created",
    "child_started",
    "agent_seal_prepared",
    "agent_seal_committed",
    "agent_result_collected",
    "agent_integration_prepared",
    "agent_integration_started",
    "agent_integration_failed",
    "agent_integration_committed",
    "child_finished",
    "agent_cleanup_prepared",
    "agent_cleanup_committed",
)
USAGE_FIELDS = (
    "input_tokens",
    "output_tokens",
    "cache_hit_tokens",
    "cache_miss_tokens",
    "cache_write_tokens",
    "reasoning_tokens",
    "reasoning_replay_tokens",
)


class EvaluationError(RuntimeError):
    """Fail-closed evaluator or evidence error."""

    def __init__(self, code: str, details: dict[str, Any] | None = None) -> None:
        super().__init__(code)
        self.code = code
        self.details = details or {}


def load_module(path: Path, name: str) -> Any:
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise EvaluationError(f"{name}_module_unavailable")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


CANARY = load_module(CANARY_PATH, "codewhale_m6_writer_canary")


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


def load_manifest() -> dict[str, Any]:
    try:
        value = json.loads(MANIFEST_PATH.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise EvaluationError("manifest_unavailable") from error
    if (
        value.get("schema") != "codewhale.eval.m6-writer-benefit-plan.v3"
        or tuple(value.get("tasks", {})) != TASK_IDS
    ):
        raise EvaluationError("manifest_invalid")
    return value


MANIFEST = load_manifest()
RESOURCES = MANIFEST["resources"]
COMMON_TOOLS = MANIFEST["treatments"]["common_allowed_tools"]


def manifest_content_hash() -> str:
    value = copy.deepcopy(MANIFEST)
    value.pop("frozen_hashes", None)
    return canonical_hash(value)


def snapshot_tree(root: Path) -> list[dict[str, Any]]:
    entries: list[dict[str, Any]] = []
    for path in sorted(root.rglob("*")):
        relative = path.relative_to(root).as_posix()
        if ".git" in path.relative_to(root).parts or "__pycache__" in path.parts:
            continue
        metadata = path.lstat()
        if stat.S_ISDIR(metadata.st_mode):
            continue
        if not stat.S_ISREG(metadata.st_mode) or path.is_symlink():
            raise EvaluationError("fixture_contains_non_regular_file")
        entries.append(
            {
                "path": relative,
                "mode": stat.S_IMODE(metadata.st_mode),
                "sha256": file_hash(path),
            }
        )
    return entries


def fixture_path(task_id: str) -> Path:
    return ROOT / MANIFEST["tasks"][task_id]["fixture"]


def fixture_hash(task_id: str) -> str:
    return canonical_hash(snapshot_tree(fixture_path(task_id)))


def verifier_file_hash(task_id: str) -> str:
    return file_hash(fixture_path(task_id) / "_eval_verifier.py")


def run_git(
    workspace: Path,
    *arguments: str,
    check: bool = True,
    environment: dict[str, str] | None = None,
    deadline: float | None = None,
) -> subprocess.CompletedProcess[str]:
    timeout = bounded_timeout(deadline, 30, "arm_harness_deadline")
    try:
        result = subprocess.run(
            ["git", *arguments],
            cwd=workspace,
            env=environment or CANARY.safe_env(),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=timeout,
            check=False,
        )
    except subprocess.TimeoutExpired as error:
        raise EvaluationError("git_deadline_exceeded") from error
    if check and result.returncode != 0:
        raise EvaluationError(
            "git_failed",
            {
                "argv_sha256": canonical_hash(arguments),
                "returncode": result.returncode,
                "stderr_sha256": sha256_bytes(result.stderr.encode()),
            },
        )
    return result


def bounded_timeout(
    deadline: float | None,
    maximum_seconds: float,
    code: str,
) -> float:
    if deadline is None:
        return maximum_seconds
    remaining = deadline - time.monotonic()
    if remaining <= 0:
        raise EvaluationError(code)
    return min(maximum_seconds, remaining)


def run_external_verifier(
    task_id: str,
    workspace: Path,
    deadline: float | None = None,
) -> dict[str, Any]:
    before = canonical_hash(snapshot_tree(workspace))
    started = time.monotonic()
    try:
        result = subprocess.run(
            [str(PYTHON), "-I", "-B", "_eval_verifier.py", "."],
            cwd=workspace,
            env=CANARY.safe_env(),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=bounded_timeout(
                deadline,
                RESOURCES["harness_external_verifier_timeout_seconds"],
                "arm_harness_deadline",
            ),
            check=False,
        )
    except subprocess.TimeoutExpired as error:
        raise EvaluationError("external_verifier_deadline_exceeded") from error
    after = canonical_hash(snapshot_tree(workspace))
    return {
        "passed": result.returncode == 0 and before == after,
        "returncode": result.returncode,
        "duration_ms": int((time.monotonic() - started) * 1000),
        "workspace_unchanged": before == after,
        "stdout_sha256": sha256_bytes(result.stdout),
        "stderr_sha256": sha256_bytes(result.stderr),
        "task_id": task_id,
    }


def initialize_workspace(
    task_id: str,
    root: Path,
    deadline: float | None = None,
) -> tuple[Path, str, str]:
    source = fixture_path(task_id)
    workspace = root / "workspace"
    shutil.copytree(source, workspace, copy_function=shutil.copy2)
    initial_tree = canonical_hash(snapshot_tree(workspace))
    if initial_tree != fixture_hash(task_id):
        raise EvaluationError("fixture_copy_mismatch")
    if run_external_verifier(task_id, workspace, deadline)["passed"]:
        raise EvaluationError("fixture_must_fail_before_task")

    run_git(workspace, "init", "-q", "-b", "main", deadline=deadline)
    run_git(workspace, "add", "--all", deadline=deadline)
    environment = {
        **CANARY.safe_env(),
        "GIT_AUTHOR_DATE": "2026-07-21T00:00:00Z",
        "GIT_COMMITTER_DATE": "2026-07-21T00:00:00Z",
    }
    run_git(
        workspace,
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "user.name=CodeWhale Eval",
        "-c",
        "user.email=eval.invalid",
        "commit",
        "-q",
        "-m",
        f"M6-B1 frozen fixture {task_id}",
        environment=environment,
        deadline=deadline,
    )
    status = run_git(
        workspace,
        "status",
        "--porcelain=v1",
        "--untracked-files=all",
        deadline=deadline,
    ).stdout
    base = run_git(
        workspace, "rev-parse", "HEAD", deadline=deadline
    ).stdout.strip()
    branch = run_git(
        workspace,
        "symbolic-ref",
        "-q",
        "HEAD",
        deadline=deadline,
    ).stdout.strip()
    if (
        status
        or len(base) != 40
        or branch != "refs/heads/main"
        or base != MANIFEST["tasks"][task_id]["fixture_base_commit"]
    ):
        raise EvaluationError("fixture_git_identity_invalid")
    return workspace, base, initial_tree


def verifier_spec(task_id: str) -> dict[str, Any]:
    name = f"m6b-{task_id}-exact"
    program = str(PYTHON.resolve())
    arguments = ["-I", "-B", "_eval_verifier.py", "."]
    command = {"name": name, "program": program, "args": arguments, "cwd": ""}
    step = {
        "id": name,
        "program": program,
        "args": arguments,
        "cwd": "",
        "env": {},
        "timeout_ms": RESOURCES["production_verifier_gate_timeout_ms"],
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


def treatment_protocol(task_id: str) -> str:
    task = MANIFEST["tasks"][task_id]
    writer_arguments = {
        "type": "implementer",
        "workspace_access": "isolated_write",
        "allowed_paths": task["allowed_paths"],
        "fork_context": False,
        "allowed_tools": WRITER_CHILD_TOOLS,
        "max_steps": 7,
        "max_depth": 0,
        "wall_time_secs": 180,
        "expected_artifact": task["expected_artifact"],
    }
    arguments = json.dumps(
        writer_arguments, ensure_ascii=False, sort_keys=True, separators=(",", ":")
    )
    return (
        "处理协议由实际工具目录决定，且是验收的一部分："
        "若当前请求没有 agent 工具，根 Agent 必须直接使用文件工具完成修改；"
        "若当前请求存在 agent 工具，根 Agent 可以读取、搜索和审查，但不得调用 "
        "apply_patch、edit_file、exec_shell、run_tests 或 run_verifiers，必须且只能调用一次 agent，"
        f"除 prompt 外的参数必须精确等于 {arguments}；prompt 必须完整转交当前任务。"
        "Writer 集成后根 Agent 只能只读审查并提出完成，最终只认 Host 冻结 verifier。"
    )


def task_definition(task_id: str) -> dict[str, Any]:
    task = MANIFEST["tasks"][task_id]
    acceptance = {
        "kind": "verifier",
        "id": f"m6b-{task_id}",
        "description": f"{task['name']} 的冻结行为、路径和工作区证据全部通过",
        "evidence_policy": (
            "failed_write_pass" if task_id == "t3" else "latest_pass"
        ),
        "verifier": verifier_spec(task_id),
    }
    return {
        "objective": f"{task['objective']}\n\n{treatment_protocol(task_id)}",
        "constraints": task["constraints"],
        "non_goals": task["non_goals"],
        "acceptance": [acceptance],
    }


def task_hash(task_id: str) -> str:
    return canonical_hash(task_definition(task_id))


def expected_writer_arguments(task_id: str) -> dict[str, Any]:
    task = MANIFEST["tasks"][task_id]
    return {
        "type": "implementer",
        "workspace_access": "isolated_write",
        "allowed_paths": task["allowed_paths"],
        "fork_context": False,
        "allowed_tools": WRITER_CHILD_TOOLS,
        "max_steps": 7,
        "max_depth": 0,
        "wall_time_secs": 180,
        "expected_artifact": task["expected_artifact"],
    }


def start_command(
    task_id: str,
    treatment: str,
    workspace: Path,
    request_id: str,
) -> dict[str, Any]:
    treatment_definition = MANIFEST["treatments"][treatment]
    limits = {
        "max_turns": RESOURCES["max_logical_model_requests_per_arm"],
        "max_model_requests": RESOURCES["max_logical_model_requests_per_arm"],
        "max_model_retries": RESOURCES["max_runtime_retries_per_arm"],
        "max_tool_calls": RESOURCES["max_tool_calls_per_arm"],
        "max_depth": treatment_definition["max_depth"],
        "max_concurrent_children": treatment_definition["max_concurrent_children"],
        "model_event_idle_ms": 90_000,
        "wall_time_ms": RESOURCES["runtime_wall_time_seconds"] * 1000,
    }
    return {
        "schema_version": RUN_API_SCHEMA,
        "request_id": request_id,
        "command": {
            "kind": "start",
            "task": task_definition(task_id),
            "workspace": str(workspace.resolve()),
            "model": MODEL,
            "reasoning_effort": RESOURCES["reasoning_effort"],
            "max_output_tokens": RESOURCES["max_output_tokens_per_request"],
            "max_api_requests": RESOURCES["max_physical_api_requests_per_arm"],
            "streaming": RESOURCES["streaming"],
            "tool_policy": {
                "enabled": True,
                "allowed": COMMON_TOOLS,
                "denied": [],
            },
            "limits": limits,
            "controls": {
                "write_execution_mode": treatment_definition[
                    "write_execution_mode"
                ],
                "auto_approve": RESOURCES["auto_approve"],
                "trust_mode": RESOURCES["trust_mode"],
                "allow_sandbox_elevation": RESOURCES[
                    "allow_sandbox_elevation"
                ],
                "interactive": RESOURCES["interactive"],
                "sandbox": RESOURCES["sandbox"],
            },
        },
    }


def frozen_start_commands() -> dict[str, dict[str, str]]:
    return {
        task_id: {
            treatment: canonical_hash(
                start_command(
                    task_id,
                    treatment,
                    Path("/workspace"),
                    f"freeze-{task_id}-{treatment}",
                )
            )
            for treatment in TREATMENTS
        }
        for task_id in TASK_IDS
    }


def frozen_app_server_argv() -> list[str]:
    return [
        "<candidate_binary>",
        "--provider",
        "deepseek",
        "app-server",
        "--stdio",
        "--transport-max-retries",
        str(RESOURCES["transport_max_retries_per_request"]),
    ]


def event_kind(stored: dict[str, Any]) -> str:
    return str(stored.get("event", {}).get("kind", ""))


def event_values(
    events: list[dict[str, Any]], name: str
) -> list[dict[str, Any]]:
    return [
        stored.get("event", {})
        for stored in events
        if event_kind(stored) == name
    ]


def stored_values(
    events: list[dict[str, Any]], name: str
) -> list[dict[str, Any]]:
    return [stored for stored in events if event_kind(stored) == name]


def strictly_increasing_sequences(values: list[dict[str, Any]]) -> bool:
    sequences = [value.get("sequence") for value in values]
    return all(isinstance(value, int) for value in sequences) and all(
        before < after
        for before, after in zip(sequences, sequences[1:])
    )


def tool_names(events: list[dict[str, Any]]) -> list[str]:
    result = []
    for event in event_values(events, "tool_prepared"):
        name = event.get("invocation", {}).get("name")
        if isinstance(name, str):
            result.append(name)
    return result


def tool_outcome_summary(events: list[dict[str, Any]]) -> dict[str, Any]:
    counts: Counter[str] = Counter()
    invocation: Counter[str] = Counter()
    operation: Counter[str] = Counter()
    retry: Counter[str] = Counter()
    verifier_verdicts: defaultdict[str, list[str]] = defaultdict(list)
    for event in event_values(events, "tool_outcome_committed"):
        name = event.get("name")
        outcome = event.get("outcome", {})
        if not isinstance(name, str) or not isinstance(outcome, dict):
            continue
        counts[name] += 1
        for target, field in (
            (invocation, "invocation"),
            (operation, "operation"),
            (retry, "retry"),
        ):
            value = outcome.get(field)
            if isinstance(value, str):
                target[f"{name}:{value}"] += 1
        observation = outcome.get("verifier_observation", {})
        verdict = observation.get("verdict") if isinstance(observation, dict) else None
        if isinstance(verdict, str):
            verifier_verdicts[name].append(verdict)
    return {
        "counts": dict(sorted(counts.items())),
        "invocation": dict(sorted(invocation.items())),
        "operation": dict(sorted(operation.items())),
        "retry": dict(sorted(retry.items())),
        "verifier_verdicts": dict(sorted(verifier_verdicts.items())),
    }


def frozen_catalog_hash(task_id: str, catalog_key: str) -> str | None:
    value = (
        MANIFEST.get("frozen_hashes", {})
        .get("ordered_tool_definition_sha256", {})
        .get(task_id, {})
        .get(catalog_key)
    )
    return value if isinstance(value, str) else None


def request_catalog_summary(
    request: Any,
    actor: str,
    source: str,
    sequence: Any,
    attempt_id: Any,
    prior_attempt_id: Any = None,
) -> dict[str, Any]:
    request = request if isinstance(request, dict) else {}
    tools = request.get("tools")
    shape_valid = isinstance(tools, list) and all(
        isinstance(tool, dict)
        and isinstance(tool.get("name"), str)
        and isinstance(tool.get("description"), str)
        and isinstance(tool.get("input_schema"), dict)
        for tool in tools
    )
    definitions = tools if shape_valid else []
    request_actor = request.get("actor", {})
    request_actor_kind = (
        request_actor.get("kind") if isinstance(request_actor, dict) else None
    )
    return {
        "actor": actor,
        "request_actor_kind": request_actor_kind,
        "source": source,
        "event_sequence": sequence,
        "attempt_id_sha256": (
            canonical_hash(attempt_id) if isinstance(attempt_id, str) else None
        ),
        "prior_attempt_id_sha256": (
            canonical_hash(prior_attempt_id)
            if isinstance(prior_attempt_id, str)
            else None
        ),
        "request_number": request.get("request_number"),
        "attempt": request.get("attempt"),
        "definition_count": len(definitions),
        "tool_names": [tool["name"] for tool in definitions],
        "shape_valid": shape_valid,
        "ordered_definitions_sha256": (
            sha256_bytes(
                json.dumps(
                    definitions,
                    ensure_ascii=False,
                    separators=(",", ":"),
                ).encode("utf-8")
            )
            if shape_valid
            else None
        ),
    }


def catalog_identity_audit(
    catalogs: list[dict[str, Any]],
    expected_catalog_hash: str | None,
    expected_empty_hash: str | None,
) -> dict[str, Any]:
    reasons: list[str] = []
    if not catalogs:
        reasons.append("catalog_requests_missing")
    if not all(
        isinstance(value, str)
        for value in (
            expected_catalog_hash,
            expected_empty_hash,
        )
    ):
        reasons.append("catalog_freeze_missing")
    groups: defaultdict[int, list[dict[str, Any]]] = defaultdict(list)
    attempt_ids: list[str] = []
    for catalog in catalogs:
        if catalog.get("shape_valid") is not True:
            reasons.append("catalog_definition_shape")
        if catalog.get("request_actor_kind") != catalog.get("actor"):
            reasons.append("catalog_actor_mismatch")
        attempt_id = catalog.get("attempt_id_sha256")
        if not prefixed_sha256(attempt_id):
            reasons.append("catalog_attempt_id")
        else:
            attempt_ids.append(attempt_id)
        request_number = catalog.get("request_number")
        attempt = catalog.get("attempt")
        if (
            not isinstance(request_number, int)
            or isinstance(request_number, bool)
            or request_number < 1
            or not isinstance(attempt, int)
            or isinstance(attempt, bool)
            or attempt < 0
        ):
            reasons.append("catalog_attempt_identity")
            continue
        groups[request_number].append(catalog)

    if len(attempt_ids) != len(set(attempt_ids)):
        reasons.append("catalog_attempt_id_reused")
    if groups and sorted(groups) != list(range(1, max(groups) + 1)):
        reasons.append("catalog_request_number_sequence")

    empty_logical_requests: list[int] = []
    for request_number, attempts in sorted(groups.items()):
        attempts.sort(key=lambda item: item["attempt"])
        if [item["attempt"] for item in attempts] != list(range(len(attempts))):
            reasons.append("catalog_attempt_sequence")
        if attempts and attempts[0].get("source") != "model_request_prepared":
            reasons.append("catalog_initial_attempt_source")
        if any(
            item.get("source") != "model_request_failed.retry.prepared"
            for item in attempts[1:]
        ):
            reasons.append("catalog_retry_attempt_source")
        for index, item in enumerate(attempts):
            expected_prior = (
                None
                if index == 0
                else attempts[index - 1].get("attempt_id_sha256")
            )
            if item.get("prior_attempt_id_sha256") != expected_prior:
                reasons.append("catalog_retry_attempt_lineage")
        definition_hashes = {
            item.get("ordered_definitions_sha256") for item in attempts
        }
        definition_counts = {item.get("definition_count") for item in attempts}
        if len(definition_hashes) != 1 or len(definition_counts) != 1:
            reasons.append("catalog_retry_drift")
            continue
        empty = definition_counts == {0}
        if empty:
            empty_logical_requests.append(request_number)
            if definition_hashes != {expected_empty_hash}:
                reasons.append("terminal_empty_catalog_hash")
        elif definition_hashes != {expected_catalog_hash}:
            reasons.append("catalog_definition_hash")

    if len(empty_logical_requests) > 1:
        reasons.append("multiple_terminal_empty_catalogs")
    if empty_logical_requests and empty_logical_requests[0] != max(groups, default=0):
        reasons.append("terminal_empty_catalog_not_last")
    return {
        "valid": not reasons,
        "reasons": list(dict.fromkeys(reasons)),
        "logical_requests": len(groups),
        "terminal_empty_logical_requests": len(empty_logical_requests),
    }


def local_temporal_lineage_audit(events: list[dict[str, Any]]) -> dict[str, Any]:
    fact = committed_receipt_fact(events)
    receipt = fact["receipt"] if fact is not None else {}
    lineage = receipt.get("lineage", {}) if isinstance(receipt, dict) else {}
    failure = lineage.get("failure", {}) if isinstance(lineage, dict) else {}
    mutation = lineage.get("mutation", {}) if isinstance(lineage, dict) else {}
    named_calls = []
    prepared_by_operation: dict[str, dict[str, Any]] = {}
    started_by_operation: dict[str, dict[str, Any]] = {}
    outcomes_by_operation: dict[str, dict[str, Any]] = {}
    for stored in events:
        event = stored.get("event", {})
        if event.get("kind") == "tool_prepared":
            operation_id = event.get("operation_id")
            if isinstance(operation_id, str):
                prepared_by_operation[operation_id] = stored
            invocation = event.get("invocation", {})
            if invocation.get("name") == "run_verifiers":
                named_calls.append(
                    invocation.get("arguments", {}).get("parsed")
                )
        elif event.get("kind") == "tool_execution_started":
            operation_id = event.get("operation_id")
            if isinstance(operation_id, str):
                started_by_operation[operation_id] = stored
        elif event.get("kind") == "tool_outcome_committed":
            operation_id = event.get("operation_id")
            if isinstance(operation_id, str):
                outcomes_by_operation[operation_id] = stored

    failure_source = failure.get("source", {}) if isinstance(failure, dict) else {}
    failure_operation_id = failure_source.get("operation_id")
    failure_prepared = prepared_by_operation.get(failure_operation_id, {})
    failure_started = started_by_operation.get(failure_operation_id, {})
    failure_committed = outcomes_by_operation.get(failure_operation_id, {})
    failure_event = failure_committed.get("event", {})
    failure_outcome = failure_event.get("outcome", {})
    failure_observation = failure_outcome.get("verifier_observation", {})

    mutation_operation_id = mutation.get("operation_id")
    mutation_prepared = prepared_by_operation.get(mutation_operation_id, {})
    mutation_started = started_by_operation.get(mutation_operation_id, {})
    mutation_committed = outcomes_by_operation.get(mutation_operation_id, {})
    mutation_prepared_event = mutation_prepared.get("event", {})
    mutation_event = mutation_committed.get("event", {})
    mutation_outcome = mutation_event.get("outcome", {})

    failure_state = failure.get("workspace_state", {})
    before = mutation.get("workspace_state_before", {})
    after = mutation.get("workspace_state_after", {})
    final_state = receipt.get("workspace_state", {})
    failure_state = failure_state if isinstance(failure_state, dict) else {}
    before = before if isinstance(before, dict) else {}
    after = after if isinstance(after, dict) else {}
    final_state = final_state if isinstance(final_state, dict) else {}
    failure_sequence = failure_committed.get("sequence")
    failure_prepared_sequence = failure_prepared.get("sequence")
    failure_started_sequence = failure_started.get("sequence")
    mutation_sequence = mutation_committed.get("sequence")
    mutation_prepared_sequence = mutation_prepared.get("sequence")
    mutation_started_sequence = mutation_started.get("sequence")
    receipt_sequence = fact["stored"].get("sequence") if fact is not None else None
    named_reference_valid = bool(named_calls) and all(
        arguments == {"verifier_id": "m6b-t3"} for arguments in named_calls
    )
    valid = (
        lineage.get("policy") == "failed_write_pass"
        and named_reference_valid
        and failure_source.get("kind") == "tool"
        and failure_started.get("event", {}).get("operation_id")
        == failure_operation_id
        and failure_prepared.get("event", {}).get("workspace_access")
        == "may_write"
        and failure_prepared.get("event", {}).get("invocation", {}).get("name")
        == "run_verifiers"
        and failure_event.get("name") == "run_verifiers"
        and failure_outcome.get("invocation") == "accepted"
        and failure_outcome.get("transport") == "succeeded"
        and failure_outcome.get("operation") == "failed"
        and failure_outcome.get("side_effect") != "applied"
        and failure_observation.get("spec") == verifier_spec("t3")
        and failure_observation.get("verdict") == "failed"
        and failure_observation.get("workspace_revision")
        == failure_state.get("revision")
        and failure_observation.get("artifact_ids")
        == failure.get("artifact_ids")
        and failure_outcome.get("evidence", {}).get("status") == "produced"
        and failure_outcome.get("evidence", {}).get("references")
        == failure.get("artifact_ids")
        and failure_event.get("workspace_state") == failure_state
        and failure_outcome.get("workspace_revision")
        == failure_state.get("revision", {}).get("sha256")
        and verification_artifacts_valid(
            failure_outcome,
            verifier_spec("t3"),
            "failed",
            failure_state.get("revision", {}),
        )
        and mutation_started.get("event", {}).get("operation_id")
        == mutation_operation_id
        and mutation_prepared_event.get("workspace_access") == "may_write"
        and mutation_prepared_event.get("invocation", {}).get("name")
        not in {None, "run_verifiers"}
        and mutation_outcome.get("invocation") == "accepted"
        and mutation_outcome.get("transport") == "succeeded"
        and mutation_outcome.get("operation") == "succeeded"
        and mutation_outcome.get("side_effect") == "applied"
        and mutation_event.get("workspace_state") == after
        and isinstance(before.get("generation"), int)
        and after.get("generation") == before.get("generation") + 1
        and before.get("revision") != after.get("revision")
        and isinstance(failure_state.get("generation"), int)
        and failure_state.get("generation") <= before.get("generation")
        and failure_state.get("revision") != final_state.get("revision")
        and after.get("revision") == final_state.get("revision")
        and isinstance(final_state.get("generation"), int)
        and final_state.get("generation") > after.get("generation")
        and isinstance(failure_sequence, int)
        and isinstance(mutation_sequence, int)
        and isinstance(receipt_sequence, int)
        and isinstance(failure_prepared_sequence, int)
        and isinstance(failure_started_sequence, int)
        and failure_prepared_sequence
        < failure_started_sequence
        < failure_sequence
        and isinstance(mutation_prepared_sequence, int)
        and isinstance(mutation_started_sequence, int)
        and mutation_prepared_sequence
        < mutation_started_sequence
        < mutation_sequence
        and failure_sequence < mutation_sequence < receipt_sequence
    )
    return {
        "valid": valid,
        "lineage_policy": lineage.get("policy"),
        "named_verifier_reference_valid": named_reference_valid,
        "named_verifier_call_count": len(named_calls),
        "failed_then_write_then_host_pass": (
            isinstance(failure_sequence, int)
            and isinstance(mutation_sequence, int)
            and isinstance(receipt_sequence, int)
            and failure_sequence < mutation_sequence < receipt_sequence
        ),
        "verifier_spec_sha256": canonical_hash(verifier_spec("t3")),
    }


def delegated_temporal_lineage_audit(
    root_events: list[dict[str, Any]], child_events: list[dict[str, Any]]
) -> dict[str, Any]:
    child_local = local_temporal_lineage_audit(child_events)
    root_fact = committed_receipt_fact(root_events)
    child_fact = committed_receipt_fact(child_events)
    root_receipt = root_fact["receipt"] if root_fact is not None else {}
    child_receipt = child_fact["receipt"] if child_fact is not None else {}
    lineage = root_receipt.get("lineage", {})
    prepared = event_values(root_events, "agent_task_prepared")
    task = prepared[0].get("task", {}) if len(prepared) == 1 else {}
    child_started = event_values(root_events, "child_started")
    child_created = run_created(child_events)
    task_prepared_stored = stored_values(root_events, "agent_task_prepared")
    child_started_stored = stored_values(root_events, "child_started")
    seal_prepared_stored = stored_values(root_events, "agent_seal_prepared")
    seal_committed_stored = stored_values(root_events, "agent_seal_committed")
    result_stored = stored_values(root_events, "agent_result_collected")
    integration_prepared_stored = stored_values(
        root_events, "agent_integration_prepared"
    )
    integration_started_stored = stored_values(
        root_events, "agent_integration_started"
    )
    integration_committed_stored = stored_values(
        root_events, "agent_integration_committed"
    )
    result_events = event_values(root_events, "agent_result_collected")
    collected_evidence = (
        result_events[0]
        .get("outcome", {})
        .get("details", {})
        .get("evidence", [])
        if len(result_events) == 1
        else []
    )
    integration_prepared = event_values(root_events, "agent_integration_prepared")
    integration_started = event_values(root_events, "agent_integration_started")
    integration_committed = event_values(root_events, "agent_integration_committed")
    prepared_event = integration_prepared[0] if len(integration_prepared) == 1 else {}
    started_event = integration_started[0] if len(integration_started) == 1 else {}
    committed_event = integration_committed[0] if len(integration_committed) == 1 else {}
    integration = lineage.get("integration", {}) if isinstance(lineage, dict) else {}
    operation_id = integration.get("operation_id")
    integration_after = integration.get("workspace_state_after", {})
    integration_before = integration.get("workspace_state_before", {})
    final_state = root_receipt.get("workspace_state", {})
    integration_after = (
        integration_after if isinstance(integration_after, dict) else {}
    )
    integration_before = (
        integration_before if isinstance(integration_before, dict) else {}
    )
    final_state = final_state if isinstance(final_state, dict) else {}

    agent_prepared = [
        stored
        for stored in stored_values(root_events, "tool_prepared")
        if stored.get("event", {}).get("invocation", {}).get("name") == "agent"
        and stored.get("event", {}).get("invocation", {}).get("call_id")
        == task.get("call_id")
    ]
    agent_operation_id = (
        agent_prepared[0].get("event", {}).get("operation_id")
        if len(agent_prepared) == 1
        else None
    )
    agent_started = [
        stored
        for stored in stored_values(root_events, "tool_execution_started")
        if stored.get("event", {}).get("operation_id") == agent_operation_id
    ]
    agent_committed = [
        stored
        for stored in stored_values(root_events, "tool_outcome_committed")
        if stored.get("event", {}).get("operation_id") == agent_operation_id
        and stored.get("event", {}).get("call_id") == task.get("call_id")
        and stored.get("event", {}).get("name") == "agent"
    ]
    agent_outcome_event = (
        agent_committed[0].get("event", {}) if len(agent_committed) == 1 else {}
    )
    agent_outcome = agent_outcome_event.get("outcome", {})
    root_receipt_sequence = (
        root_fact["stored"].get("sequence") if root_fact is not None else None
    )
    integration_sequence = (
        integration_committed_stored[0].get("sequence")
        if len(integration_committed_stored) == 1
        else None
    )
    agent_outcome_sequence = (
        agent_committed[0].get("sequence") if len(agent_committed) == 1 else None
    )
    root_receipt_stored = root_fact["stored"] if root_fact is not None else {}
    valid = (
        child_local["valid"]
        and lineage.get("policy") == "delegated_failed_write_pass"
        and child_receipt.get("lineage", {}).get("policy") == "failed_write_pass"
        and lineage.get("child_run_id") == task.get("child_run_id")
        and len(child_started) == 1
        and child_started[0].get("child_run_id") == task.get("child_run_id")
        and child_created.get("run_id") == task.get("child_run_id")
        and len(task_prepared_stored) == 1
        and len(child_started_stored) == 1
        and len(seal_prepared_stored) == 1
        and len(seal_committed_stored) == 1
        and len(result_stored) == 1
        and len(integration_prepared_stored) == 1
        and len(integration_started_stored) == 1
        and len(integration_committed_stored) == 1
        and lineage.get("child_receipt_id") == child_receipt.get("id")
        and collected_evidence == [child_receipt]
        and prepared_event.get("integration_id") == operation_id
        and started_event.get("integration_id") == operation_id
        and committed_event.get("integration_id") == operation_id
        and integration.get("workspace_state_before")
        == prepared_event.get("expected_root_workspace_state")
        and integration_after == committed_event.get("root_workspace_state_after")
        and isinstance(integration_before.get("generation"), int)
        and integration_after.get("generation")
        == integration_before.get("generation") + 1
        and integration_after.get("revision")
        != integration_before.get("revision")
        and len(agent_prepared) == 1
        and len(agent_started) == 1
        and len(agent_committed) == 1
        and agent_started[0].get("event", {}).get("operation_id")
        == agent_operation_id
        and agent_outcome_event.get("workspace_state") == integration_after
        and agent_outcome.get("invocation") == "accepted"
        and agent_outcome.get("transport") == "succeeded"
        and agent_outcome.get("operation") == "succeeded"
        and agent_outcome.get("side_effect") == "applied"
        and isinstance(integration_after.get("generation"), int)
        and isinstance(final_state.get("generation"), int)
        and final_state.get("generation") > integration_after.get("generation")
        and final_state.get("revision") == integration_after.get("revision")
        and isinstance(integration_sequence, int)
        and isinstance(agent_outcome_sequence, int)
        and isinstance(root_receipt_sequence, int)
        and integration_sequence
        < agent_outcome_sequence
        < root_receipt_sequence
        and strictly_increasing_sequences(
            [
                agent_prepared[0],
                agent_started[0],
                task_prepared_stored[0],
                child_started_stored[0],
                seal_prepared_stored[0],
                seal_committed_stored[0],
                result_stored[0],
                integration_prepared_stored[0],
                integration_started_stored[0],
                integration_committed_stored[0],
                agent_committed[0],
                root_receipt_stored,
            ]
        )
    )
    return {
        "valid": valid,
        "lineage_policy": lineage.get("policy"),
        "child": child_local,
        "child_receipt_linked": lineage.get("child_receipt_id")
        == child_receipt.get("id"),
        "integration_linked": (
            prepared_event.get("integration_id")
            == started_event.get("integration_id")
            == committed_event.get("integration_id")
            == operation_id
        ),
    }


def t3_recovery_audit(
    treatment: str,
    root_events: list[dict[str, Any]],
    child_events: list[dict[str, Any]],
) -> dict[str, Any]:
    if treatment == "writer":
        return delegated_temporal_lineage_audit(root_events, child_events)
    return local_temporal_lineage_audit(root_events)


def catalog_summary(
    events: list[dict[str, Any]], actor: str
) -> list[dict[str, Any]]:
    result = []
    for stored in events:
        event = stored.get("event", {})
        kind = event.get("kind") if isinstance(event, dict) else None
        if kind == "model_request_prepared":
            result.append(
                request_catalog_summary(
                    event.get("request"),
                    actor,
                    "model_request_prepared",
                    stored.get("sequence"),
                    event.get("attempt_id"),
                )
            )
        elif kind == "model_request_failed":
            retry = event.get("retry", {})
            prepared = retry.get("prepared", {}) if isinstance(retry, dict) else {}
            if retry.get("decision") == "retry" and isinstance(prepared, dict):
                result.append(
                    request_catalog_summary(
                        prepared.get("request"),
                        actor,
                        "model_request_failed.retry.prepared",
                        stored.get("sequence"),
                        prepared.get("attempt_id"),
                        event.get("attempt_id"),
                    )
                )
    return result


def model_failure_summary(
    events: list[dict[str, Any]], actor: str
) -> list[dict[str, Any]]:
    failures = []
    for event in event_values(events, "model_request_failed"):
        failure = event.get("failure", {})
        retry = event.get("retry", {})
        prepared = retry.get("prepared", {}) if isinstance(retry, dict) else {}
        retry_catalog = None
        if retry.get("decision") == "retry" and isinstance(prepared, dict):
            retry_catalog = request_catalog_summary(
                prepared.get("request"),
                actor,
                "model_request_failed.retry.prepared",
                None,
                prepared.get("attempt_id"),
                event.get("attempt_id"),
            )
        failures.append(
            {
                "code": failure.get("code"),
                "category": failure.get("category"),
                "retryable": failure.get("retryable"),
                "actionable_output": failure.get("actionable_output"),
                "retry_decision": retry.get("decision"),
                "retry_stop_reason": retry.get("reason"),
                "retry_catalog": retry_catalog,
            }
        )
    return failures


def usage_zero() -> dict[str, int]:
    return {field: 0 for field in USAGE_FIELDS}


def add_usage(target: dict[str, int], value: dict[str, Any]) -> None:
    for field in USAGE_FIELDS:
        raw = value.get(field, 0)
        if isinstance(raw, int) and not isinstance(raw, bool):
            target[field] += raw


def actor_usage(events: list[dict[str, Any]]) -> dict[str, Any]:
    result = usage_zero()
    response_count = 0
    shape_valid = True
    for event in event_values(events, "model_response_committed"):
        response_count += 1
        output = event.get("output", {})
        usage = output.get("usage", {}) if isinstance(output, dict) else {}
        if (
            isinstance(usage, dict)
            and all(
                isinstance(usage.get(field), int)
                and not isinstance(usage.get(field), bool)
                and usage.get(field) >= 0
                for field in USAGE_FIELDS
            )
        ):
            add_usage(result, usage)
        else:
            shape_valid = False
    return {
        "usage": result,
        "response_count": response_count,
        "shape_valid": shape_valid,
    }


def summarize_accounting(value: Any) -> dict[str, Any]:
    value = value if isinstance(value, dict) else {}
    usage_value = value.get("usage")
    usage = usage_value if isinstance(usage_value, dict) else {}
    root_value = value.get("root")
    root = root_value if isinstance(root_value, dict) else {}
    child_value = value.get("child")
    child = child_value if isinstance(child_value, dict) else {}
    cost_nanousd = value.get("cost_nanousd")
    cost_nanocny = value.get("cost_nanocny")
    exact_billing = (
        value.get("sealed") is True
        and value.get("complete") is True
        and value.get("usage_complete") is True
        and value.get("usage_missing") is False
        and value.get("usage_incomplete") is False
        and value.get("records_after_seal") == 0
        and value.get("billing_unknown") is False
        and value.get("unpriced") is False
        and value.get("usage_missing_responses") == 0
        and value.get("incomplete_responses") == 0
        and value.get("billing_unknown_attempts") == 0
        and value.get("unpriced_usage_responses") == 0
        and isinstance(cost_nanousd, int)
        and not isinstance(cost_nanousd, bool)
        and isinstance(cost_nanocny, int)
        and not isinstance(cost_nanocny, bool)
    )
    request_count_unknown = any(
        not isinstance(counter, int) or isinstance(counter, bool)
        for actor in (root, child)
        for counter in (
            actor.get("started"),
            actor.get("completed"),
            actor.get("in_flight"),
            actor.get("retries"),
        )
    )
    summary = {
        "hard_request_limit": value.get("hard_request_limit"),
        "root": {
            key: root.get(key)
            for key in ("started", "completed", "in_flight", "retries")
        },
        "child": {
            key: child.get(key)
            for key in ("started", "completed", "in_flight", "retries")
        },
        "transport_retries": value.get("transport_retries"),
        "runtime_retries": value.get("runtime_retries"),
        "sealed_denied": value.get("sealed_denied"),
        "exhausted_denied": value.get("exhausted_denied"),
        "budget_exhausted": value.get("budget_exhausted"),
        "sealed": value.get("sealed"),
        "complete": value.get("complete"),
        "usage_complete": value.get("usage_complete"),
        "usage_missing": value.get("usage_missing"),
        "usage_incomplete": value.get("usage_incomplete"),
        "billing_unknown": not exact_billing,
        "known_cost_is_lower_bound": not exact_billing,
        "request_count_unknown": request_count_unknown,
        "unpriced": value.get("unpriced"),
        "usage_responses": value.get("usage_responses"),
        "usage_missing_responses": value.get("usage_missing_responses"),
        "incomplete_responses": value.get("incomplete_responses"),
        "billing_unknown_attempts": value.get("billing_unknown_attempts"),
        "unpriced_usage_responses": value.get("unpriced_usage_responses"),
        "records_after_seal": value.get("records_after_seal"),
        "usage": {
            field: usage.get(field)
            for field in USAGE_FIELDS
        },
        "surface_usage": [],
        "cost_nanousd": cost_nanousd,
        "cost_nanocny": cost_nanocny,
    }
    surface_values = value.get("surface_usage")
    for surface in surface_values if isinstance(surface_values, list) else []:
        if not isinstance(surface, dict):
            summary["surface_usage"].append({"invalid": True})
            continue
        surface_usage = surface.get("usage", {})
        summary["surface_usage"].append(
            {
                "surface": surface.get("surface"),
                "model": surface.get("model"),
                "response_count": surface.get("response_count"),
                "usage_response_count": surface.get("usage_response_count"),
                "usage": {
                    field: surface_usage.get(field)
                    for field in USAGE_FIELDS
                }
                if isinstance(surface_usage, dict)
                else {},
                "cost_nanousd": surface.get("cost_nanousd"),
                "cost_nanocny": surface.get("cost_nanocny"),
            }
        )
    return summary


def accounting_summary(run: dict[str, Any]) -> dict[str, Any]:
    return summarize_accounting(run.get("accounting"))


def lower_bound_accounting_summary(
    summary: dict[str, Any],
    *other_summaries: dict[str, Any],
) -> dict[str, Any]:
    summary = copy.deepcopy(summary)
    for other in other_summaries:
        for field in (
            "transport_retries",
            "runtime_retries",
            "sealed_denied",
            "exhausted_denied",
            "usage_responses",
            "usage_missing_responses",
            "incomplete_responses",
            "billing_unknown_attempts",
            "unpriced_usage_responses",
            "records_after_seal",
            "cost_nanousd",
            "cost_nanocny",
        ):
            candidates = [
                value
                for value in (summary.get(field), other.get(field))
                if isinstance(value, int) and not isinstance(value, bool)
            ]
            if candidates:
                summary[field] = max(candidates)
        for actor_name in ("root", "child"):
            actor = summary.get(actor_name, {})
            other_actor = other.get(actor_name, {})
            for field in ("started", "completed", "in_flight", "retries"):
                candidates = [
                    value
                    for value in (actor.get(field), other_actor.get(field))
                    if isinstance(value, int) and not isinstance(value, bool)
                ]
                if candidates:
                    actor[field] = max(candidates)
        usage = summary.get("usage", {})
        other_usage = other.get("usage", {})
        for field in USAGE_FIELDS:
            candidates = [
                value
                for value in (usage.get(field), other_usage.get(field))
                if isinstance(value, int) and not isinstance(value, bool)
            ]
            if candidates:
                usage[field] = max(candidates)
    summary["sealed"] = False
    summary["complete"] = False
    summary["usage_complete"] = False
    summary["usage_incomplete"] = True
    summary["billing_unknown"] = True
    summary["known_cost_is_lower_bound"] = True
    summary["request_count_unknown"] = True
    return summary


def provisional_accounting_summary(run: dict[str, Any]) -> dict[str, Any]:
    return lower_bound_accounting_summary(accounting_summary(run))


def provisional_accounting_provenance(run: dict[str, Any]) -> dict[str, Any]:
    return {
        "source": "run_view_partial",
        "terminal_sequence": None,
        "run_last_sequence": run.get("last_sequence"),
        "terminal_count": 0,
        "terminal_is_last": False,
        "terminal_matches_run_view": False,
        "terminal_accounting_matches_run_view": False,
    }


def canonical_terminal_identity(
    run: dict[str, Any],
    events: list[dict[str, Any]],
) -> tuple[dict[str, Any] | None, dict[str, Any], list[str]]:
    terminals = [
        stored for stored in events if event_kind(stored) == "terminal"
    ]
    provenance = {
        "source": "canonical_terminal_event" if len(terminals) == 1 else "run_view_partial",
        "terminal_sequence": (
            terminals[0].get("sequence") if len(terminals) == 1 else None
        ),
        "run_last_sequence": run.get("last_sequence"),
        "terminal_count": len(terminals),
        "terminal_is_last": bool(terminals and events and terminals[0] == events[-1]),
        "terminal_matches_run_view": False,
        "terminal_accounting_matches_run_view": False,
    }
    reasons: list[str] = []
    if len(terminals) != 1:
        reasons.append("terminal_accounting_provenance")
        return None, provenance, reasons
    terminal = terminals[0]
    outcome = terminal.get("event", {}).get("outcome", {})
    terminal_value = outcome.get("terminal") if isinstance(outcome, dict) else None
    provenance["terminal_matches_run_view"] = (
        isinstance(outcome, dict)
        and outcome.get("run_id") == run.get("run_id")
        and terminal_value == run.get("terminal")
    )
    if (
        terminal != events[-1]
        or terminal.get("sequence") != run.get("last_sequence")
        or not provenance["terminal_matches_run_view"]
    ):
        reasons.append("terminal_accounting_provenance")
    return terminal, provenance, reasons


def canonical_terminal_accounting(
    run: dict[str, Any],
    events: list[dict[str, Any]],
) -> tuple[dict[str, Any], dict[str, Any], list[str]]:
    terminal, provenance, reasons = canonical_terminal_identity(run, events)
    if terminal is None:
        terminal_summaries = []
        for stored in events:
            if event_kind(stored) != "terminal":
                continue
            outcome = stored.get("event", {}).get("outcome", {})
            value = outcome.get("accounting") if isinstance(outcome, dict) else None
            if isinstance(value, dict):
                terminal_summaries.append(summarize_accounting(value))
        accounting = lower_bound_accounting_summary(
            accounting_summary(run),
            *terminal_summaries,
        )
        return accounting, provenance, reasons
    outcome = terminal.get("event", {}).get("outcome", {})
    accounting_value = outcome.get("accounting") if isinstance(outcome, dict) else None
    if not isinstance(accounting_value, dict):
        reasons.append("terminal_accounting_missing")
        provenance["source"] = "run_view_partial"
        return provisional_accounting_summary(run), provenance, reasons
    accounting = summarize_accounting(accounting_value)
    run_accounting = accounting_summary(run)
    provenance["terminal_accounting_matches_run_view"] = accounting == run_accounting
    if not provenance["terminal_accounting_matches_run_view"]:
        reasons.append("terminal_accounting_run_view_mismatch")
    if reasons:
        provenance["source"] = "terminal_event_unverified"
        accounting = lower_bound_accounting_summary(
            accounting,
            run_accounting,
        )
    return accounting, provenance, reasons


def is_nonnegative_int(value: Any) -> bool:
    return (
        isinstance(value, int)
        and not isinstance(value, bool)
        and value >= 0
    )


def nonnegative_int_or_zero(value: Any) -> int:
    return value if is_nonnegative_int(value) else 0


def accounting_valid(accounting: dict[str, Any]) -> tuple[bool, list[str]]:
    reasons = []
    root = accounting["root"]
    child = accounting["child"]
    actor_counter_values = [
        actor.get(field)
        for actor in (root, child)
        for field in ("started", "completed", "in_flight", "retries")
    ]
    scalar_counter_fields = (
        "transport_retries",
        "runtime_retries",
        "sealed_denied",
        "exhausted_denied",
        "usage_responses",
        "usage_missing_responses",
        "incomplete_responses",
        "billing_unknown_attempts",
        "unpriced_usage_responses",
        "records_after_seal",
        "cost_nanousd",
        "cost_nanocny",
    )
    scalar_counter_values = [
        accounting.get(field) for field in scalar_counter_fields
    ]
    usage_values = [
        accounting.get("usage", {}).get(field)
        for field in USAGE_FIELDS
    ]
    surfaces = accounting.get("surface_usage", [])
    surface_shape_valid = isinstance(surfaces, list) and all(
        isinstance(surface, dict)
        and all(
            is_nonnegative_int(surface.get(field))
            for field in (
                "response_count",
                "usage_response_count",
                "cost_nanousd",
                "cost_nanocny",
            )
        )
        and isinstance(surface.get("usage"), dict)
        and all(
            is_nonnegative_int(surface["usage"].get(field))
            for field in USAGE_FIELDS
        )
        for surface in surfaces
    )
    numeric_shape_valid = (
        all(is_nonnegative_int(value) for value in actor_counter_values)
        and all(is_nonnegative_int(value) for value in scalar_counter_values)
        and all(is_nonnegative_int(value) for value in usage_values)
        and surface_shape_valid
    )
    boolean_fields = (
        "budget_exhausted",
        "sealed",
        "complete",
        "usage_complete",
        "usage_missing",
        "usage_incomplete",
        "billing_unknown",
        "known_cost_is_lower_bound",
        "request_count_unknown",
        "unpriced",
    )
    boolean_shape_valid = all(
        isinstance(accounting.get(field), bool) for field in boolean_fields
    )
    started = nonnegative_int_or_zero(root.get("started")) + (
        nonnegative_int_or_zero(child.get("started"))
    )
    completed = nonnegative_int_or_zero(root.get("completed")) + (
        nonnegative_int_or_zero(child.get("completed"))
    )
    in_flight = nonnegative_int_or_zero(root.get("in_flight")) + (
        nonnegative_int_or_zero(child.get("in_flight"))
    )
    surface_usage = usage_zero()
    if surface_shape_valid:
        for surface in surfaces:
            add_usage(surface_usage, surface["usage"])
    surface_response_count = sum(
        nonnegative_int_or_zero(surface.get("response_count"))
        for surface in surfaces
        if isinstance(surface, dict)
    )
    surface_usage_response_count = sum(
        nonnegative_int_or_zero(surface.get("usage_response_count"))
        for surface in surfaces
        if isinstance(surface, dict)
    )
    surface_cost_nanousd = sum(
        nonnegative_int_or_zero(surface.get("cost_nanousd"))
        for surface in surfaces
        if isinstance(surface, dict)
    )
    surface_cost_nanocny = sum(
        nonnegative_int_or_zero(surface.get("cost_nanocny"))
        for surface in surfaces
        if isinstance(surface, dict)
    )
    checks = {
        "numeric_shape": numeric_shape_valid,
        "boolean_shape": boolean_shape_valid,
        "hard_request_limit": accounting["hard_request_limit"]
        == RESOURCES["max_physical_api_requests_per_arm"],
        "request_range": 1 <= started
        <= RESOURCES["max_physical_api_requests_per_arm"],
        "requests_closed": started == completed and in_flight == 0,
        "actor_requests_closed": all(
            nonnegative_int_or_zero(actor.get("started"))
            == nonnegative_int_or_zero(actor.get("completed"))
            and nonnegative_int_or_zero(actor.get("in_flight")) == 0
            for actor in (root, child)
        ),
        "response_count_within_requests": nonnegative_int_or_zero(
            accounting["usage_responses"]
        )
        <= completed,
        "retry_actor_total": (
            nonnegative_int_or_zero(root.get("retries"))
            + nonnegative_int_or_zero(child.get("retries"))
            == nonnegative_int_or_zero(accounting["transport_retries"])
        ),
        "retry_count_range": all(
            nonnegative_int_or_zero(actor.get("retries"))
            <= nonnegative_int_or_zero(actor.get("started"))
            for actor in (root, child)
        ),
        "request_count_known": accounting["request_count_unknown"] is False,
        "sealed": accounting["sealed"] is True,
        "complete": accounting["complete"] is True,
        "usage_complete": accounting["usage_complete"] is True,
        "usage_present": accounting["usage_missing"] is False,
        "usage_not_incomplete": accounting["usage_incomplete"] is False,
        "billing_known": accounting["billing_unknown"] is False,
        "priced": accounting["unpriced"] is False,
        "usage_gap_counters_zero": (
            accounting["usage_missing_responses"] == 0
            and accounting["incomplete_responses"] == 0
            and accounting["billing_unknown_attempts"] == 0
            and accounting["unpriced_usage_responses"] == 0
        ),
        "sealed_denied_zero": accounting["sealed_denied"] == 0,
        "no_records_after_seal": accounting["records_after_seal"] == 0,
        "transport_retry_limit": nonnegative_int_or_zero(
            accounting["transport_retries"]
        )
        <= RESOURCES["accepted_transport_retries_per_arm"],
        "runtime_retry_limit": nonnegative_int_or_zero(
            accounting["runtime_retries"]
        )
        <= RESOURCES["max_runtime_retries_per_arm"],
        "budget_exhaustion_semantics": (
            accounting["budget_exhausted"] is True
        ) == (
            nonnegative_int_or_zero(accounting["exhausted_denied"]) > 0
        ),
        "cost_available": is_nonnegative_int(accounting["cost_nanousd"])
        and is_nonnegative_int(accounting["cost_nanocny"]),
        "surface_usage_matches_total": (
            surface_shape_valid
            and surface_usage == accounting["usage"]
            and surface_response_count == accounting["usage_responses"]
            and surface_usage_response_count == accounting["usage_responses"]
            and surface_cost_nanousd == accounting["cost_nanousd"]
            and surface_cost_nanocny == accounting["cost_nanocny"]
        ),
        "standard_chat_only": bool(surfaces)
        and all(
            surface.get("surface") == "standard_chat"
            and surface.get("model") == MODEL
            for surface in surfaces
            if isinstance(surface, dict)
        ),
    }
    reasons.extend(name for name, passed in checks.items() if not passed)
    return not reasons, reasons


def run_created(events: list[dict[str, Any]]) -> dict[str, Any]:
    values = event_values(events, "run_created")
    return values[0].get("request", {}) if len(values) == 1 else {}


def state_schema_summary(codewhale_home: Path) -> dict[str, Any]:
    database = codewhale_home / "state.db"
    try:
        connection = sqlite3.connect(f"file:{database}?mode=ro", uri=True)
        try:
            row = connection.execute("PRAGMA user_version").fetchone()
        finally:
            connection.close()
        version = row[0] if row and len(row) == 1 else None
        return {
            "valid": version == STATE_SCHEMA,
            "user_version": version,
            "database_sha256": file_hash(database),
        }
    except (OSError, sqlite3.Error) as error:
        raise EvaluationError("state_schema_unavailable") from error


def typed_event_terminal_state(
    events: list[dict[str, Any]]
) -> str | None:
    values = event_values(events, "terminal")
    if len(values) != 1:
        return None
    terminal = values[0].get("outcome", {}).get("terminal", {})
    return terminal.get("state") if isinstance(terminal, dict) else None


def committed_receipt_fact(
    events: list[dict[str, Any]],
) -> dict[str, Any] | None:
    facts = [
        {"stored": stored, "event": stored.get("event", {}), "receipt": receipt}
        for stored in events
        if event_kind(stored) == "host_verification_committed"
        and isinstance((receipt := stored.get("event", {}).get("receipt")), dict)
    ]
    return facts[0] if len(facts) == 1 else None


def verification_artifacts_valid(
    outcome: dict[str, Any],
    verifier: dict[str, Any],
    verdict: str,
    workspace_revision: dict[str, Any],
) -> bool:
    observation = outcome.get("verifier_observation", {})
    artifacts = outcome.get("artifacts", [])
    artifact_ids = observation.get("artifact_ids", [])
    if (
        not isinstance(artifacts, list)
        or not artifacts
        or not isinstance(artifact_ids, list)
        or [artifact.get("id") for artifact in artifacts] != artifact_ids
    ):
        return False
    for artifact in artifacts:
        if not isinstance(artifact, dict):
            return False
        content = artifact.get("inline_content")
        if not isinstance(content, dict):
            return False
        encoded = canonical_bytes(content)
        digest = sha256_bytes(encoded)
        if (
            artifact.get("status") != "available"
            or artifact.get("id") != f"verification-evidence:{digest}"
            or artifact.get("sha256") != digest
            or artifact.get("media_type")
            != "application/vnd.codewhale.verification+json"
            or artifact.get("byte_len") != len(encoded)
            or content.get("verifier") != verifier
            or content.get("verdict") != verdict
            or content.get("workspace_revision") != workspace_revision
            or not isinstance(content.get("summary"), str)
            or not content["summary"].strip()
        ):
            return False
    return True


def completion_receipt_summary(
    task_id: str,
    events: list[dict[str, Any]],
    terminal_state: str | None,
) -> dict[str, Any]:
    committed = event_values(events, "host_verification_committed")
    receipts = [
        event.get("receipt")
        for event in committed
        if isinstance(event.get("receipt"), dict)
    ]
    terminal_events = event_values(events, "terminal")
    terminal = (
        terminal_events[0].get("outcome", {}).get("terminal", {})
        if len(terminal_events) == 1
        else {}
    )
    valid = False
    receipt_hash = None
    workspace_state = None
    lineage_policy = None
    fact = committed_receipt_fact(events)
    if fact is not None:
        event = fact["event"]
        receipt = fact["receipt"]
        receipt_hash = canonical_hash(receipt)
        workspace_state = receipt.get("workspace_state")
        lineage = receipt.get("lineage", {})
        lineage_policy = (
            lineage.get("policy") if isinstance(lineage, dict) else None
        )
        created = run_created(events)
        contract = created.get("task_contract", {})
        definition = contract.get("definition", {})
        acceptance = definition.get("acceptance", [])
        expected_acceptance = acceptance[0] if len(acceptance) == 1 else {}
        outcome = event.get("outcome", {})
        observation = (
            outcome.get("verifier_observation", {})
            if isinstance(outcome, dict)
            else {}
        )
        verification_id = event.get("verification_id")
        matching_prepared = [
            stored
            for stored in events
            if event_kind(stored) == "host_verification_prepared"
            and stored.get("event", {}).get("verification_id") == verification_id
        ]
        matching_started = [
            stored
            for stored in events
            if event_kind(stored) == "host_verification_started"
            and stored.get("event", {}).get("verification_id") == verification_id
        ]
        decision = terminal.get("decision", {}) if isinstance(terminal, dict) else {}
        satisfied = decision.get("satisfied", []) if isinstance(decision, dict) else []
        prepared_state = (
            matching_prepared[0]
            .get("event", {})
            .get("workspace_state_before")
            if len(matching_prepared) == 1
            else None
        )
        revision = (
            workspace_state.get("revision")
            if isinstance(workspace_state, dict)
            else None
        )
        valid = (
            terminal_state == "completed"
            and expected_acceptance.get("kind") == "verifier"
            and expected_acceptance.get("id") == f"m6b-{task_id}"
            and expected_acceptance.get("evidence_policy")
            == ("failed_write_pass" if task_id == "t3" else "latest_pass")
            and expected_acceptance.get("verifier") == verifier_spec(task_id)
            and len(matching_prepared) == 1
            and len(matching_started) == 1
            and matching_prepared[0].get("sequence", 0)
            < matching_started[0].get("sequence", 0)
            < fact["stored"].get("sequence", 0)
            and event.get("workspace_state_after") == workspace_state
            and isinstance(prepared_state, dict)
            and isinstance(prepared_state.get("generation"), int)
            and isinstance(workspace_state, dict)
            and workspace_state.get("generation")
            == prepared_state.get("generation") + 1
            and workspace_state.get("revision")
            == prepared_state.get("revision")
            and isinstance(revision, dict)
            and revision.get("status") == "known"
            and receipt.get("generation_id") == contract.get("generation_id")
            and receipt.get("acceptance_id") == expected_acceptance.get("id")
            and receipt.get("verification_id") == verification_id
            and receipt.get("id") == f"receipt:{verification_id}"
            and receipt.get("verifier") == verifier_spec(task_id)
            and isinstance(receipt.get("artifact_ids"), list)
            and bool(receipt.get("artifact_ids"))
            and observation.get("spec") == verifier_spec(task_id)
            and observation.get("verdict") == "passed"
            and observation.get("workspace_revision") == revision
            and observation.get("artifact_ids") == receipt.get("artifact_ids")
            and outcome.get("workspace_revision") == revision.get("sha256")
            and outcome.get("invocation") == "accepted"
            and outcome.get("transport") == "succeeded"
            and outcome.get("operation") == "succeeded"
            and outcome.get("side_effect") != "applied"
            and outcome.get("evidence", {}).get("status") == "produced"
            and outcome.get("evidence", {}).get("references")
            == receipt.get("artifact_ids")
            and verification_artifacts_valid(
                outcome, verifier_spec(task_id), "passed", revision
            )
            and len(satisfied) == 1
            and satisfied[0].get("kind") == "evidence"
            and satisfied[0].get("acceptance_id")
            == expected_acceptance.get("id")
            and satisfied[0].get("receipt_id") == receipt.get("id")
            and decision.get("generation_id") == contract.get("generation_id")
            and decision.get("workspace_state") == workspace_state
        )
    return {
        "valid": valid,
        "receipt_count": len(receipts),
        "receipt_sha256": receipt_hash,
        "workspace_state": workspace_state,
        "lineage_policy": lineage_policy,
        "completion_rejections": len(event_values(events, "completion_rejected")),
    }


def raw_sha256(value: Any) -> bool:
    return (
        isinstance(value, str)
        and len(value) == 64
        and all(character in "0123456789abcdef" for character in value)
    )


def writer_path_set_sha256(paths: list[str]) -> str:
    canonical = sorted(set(paths))
    return hashlib.sha256(
        json.dumps(
            canonical, ensure_ascii=False, separators=(",", ":")
        ).encode("utf-8")
    ).hexdigest()


def expected_writer_cleanup_context(
    root_events: list[dict[str, Any]],
) -> tuple[str, str]:
    if event_values(root_events, "agent_integration_committed"):
        return "post_integration", "writer_integrated"
    failures = event_values(root_events, "agent_integration_failed")
    if failures:
        status = failures[-1].get("status", {})
        state = status.get("state") if isinstance(status, dict) else None
        reason = {
            "rejected": "writer_integration_rejected",
            "conflict": "writer_integration_conflict",
            "recovery_required": "writer_integration_recovery_required",
        }.get(state, "writer_integration_invalid")
        return "integration", reason
    if not event_values(root_events, "child_started"):
        return "binding", "writer_binding_failed"
    if event_values(root_events, "agent_seal_prepared"):
        return "seal", "writer_seal_failed"
    results = event_values(root_events, "agent_result_collected")
    terminal = (
        results[-1].get("outcome", {}).get("terminal", {})
        if results
        else {}
    )
    state = terminal.get("state") if isinstance(terminal, dict) else None
    reason = {
        "blocked": "writer_child_blocked",
        "cancelled": "writer_child_cancelled",
        "interrupted": "writer_child_interrupted",
        "recovery_required": "writer_child_recovery_required",
    }.get(state, "writer_child_failed")
    return "child", reason


def writer_cleanup_summary(
    canonical_task_id: str,
    root_events: list[dict[str, Any]],
    task: dict[str, Any],
    seal_event: dict[str, Any],
    integration_event: dict[str, Any],
) -> dict[str, Any]:
    identity_reasons: list[str] = []
    prepared = event_values(root_events, "agent_cleanup_prepared")
    committed = event_values(root_events, "agent_cleanup_committed")
    plan = prepared[0].get("plan", {}) if len(prepared) == 1 else {}
    result = committed[0].get("result", {}) if len(committed) == 1 else {}
    if len(prepared) != 1:
        identity_reasons.append("writer_cleanup_plan_cardinality")
    if len(committed) != 1:
        identity_reasons.append("writer_cleanup_result_cardinality")
    if not isinstance(plan, dict):
        plan = {}
        identity_reasons.append("writer_cleanup_plan_shape")
    if not isinstance(result, dict):
        result = {}
        identity_reasons.append("writer_cleanup_result_shape")

    ownership = plan.get("ownership", {})
    artifact = plan.get("artifact_state", {})
    scope = plan.get("scope", {})
    mode = plan.get("mode", {})
    ownership_state = ownership.get("state") if isinstance(ownership, dict) else None
    artifact_state = artifact.get("state") if isinstance(artifact, dict) else None
    scope_state = scope.get("state") if isinstance(scope, dict) else None
    mode_name = mode.get("mode") if isinstance(mode, dict) else None
    uncertainty_codes = []
    for value in (ownership, artifact, scope, mode, result):
        if isinstance(value, dict) and isinstance(value.get("uncertainty_code"), str):
            uncertainty_codes.append(value["uncertainty_code"])

    if ownership_state == "known":
        if not raw_sha256(ownership.get("identity_sha256")):
            identity_reasons.append("writer_cleanup_ownership_hash")
    elif ownership_state == "unknown":
        if not isinstance(ownership.get("uncertainty_code"), str):
            identity_reasons.append("writer_cleanup_ownership_uncertainty")
    else:
        identity_reasons.append("writer_cleanup_ownership_shape")

    changed_files = seal_event.get("changed_files", [])
    if not isinstance(changed_files, list) or not all(
        isinstance(path, str) for path in changed_files
    ):
        changed_files = []
        if seal_event:
            identity_reasons.append("writer_seal_changed_files_shape")
    allowed_paths = task.get("workspace", {}).get("allowed_paths", [])
    if scope_state == "known":
        revision = scope.get("workspace_revision", {})
        counts = [
            scope.get("changed_count"),
            scope.get("in_scope_count"),
            scope.get("out_of_scope_count"),
        ]
        if (
            not isinstance(revision, dict)
            or revision.get("status") != "known"
            or not raw_sha256(revision.get("sha256"))
        ):
            identity_reasons.append("writer_cleanup_scope_revision")
        if not all(
            isinstance(value, int) and not isinstance(value, bool) and value >= 0
            for value in counts
        ) or counts[1] + counts[2] != counts[0]:
            identity_reasons.append("writer_cleanup_scope_counts")
        if not raw_sha256(scope.get("path_set_sha256")):
            identity_reasons.append("writer_cleanup_scope_hash")
        if seal_event:
            expected_in_scope = sum(path in allowed_paths for path in changed_files)
            if (
                scope.get("changed_count") != len(changed_files)
                or scope.get("in_scope_count") != expected_in_scope
                or scope.get("out_of_scope_count")
                != len(changed_files) - expected_in_scope
                or scope.get("path_set_sha256")
                != writer_path_set_sha256(changed_files)
            ):
                identity_reasons.append("writer_cleanup_scope_seal_mismatch")
    elif scope_state == "unknown":
        if not isinstance(scope.get("uncertainty_code"), str):
            identity_reasons.append("writer_cleanup_scope_uncertainty")
    else:
        identity_reasons.append("writer_cleanup_scope_shape")

    final_commit = seal_event.get("final_commit")
    diff_sha256 = seal_event.get("diff_sha256")
    if seal_event:
        if (
            artifact_state != "known_host_sealed"
            or not isinstance(final_commit, str)
            or len(final_commit) != 40
            or any(character not in "0123456789abcdef" for character in final_commit)
            or not raw_sha256(diff_sha256)
            or artifact.get("final_commit") != final_commit
            or artifact.get("diff_sha256") != diff_sha256
        ):
            identity_reasons.append("writer_cleanup_sealed_artifact")
    elif artifact_state == "known_unsealed":
        pass
    elif artifact_state == "unknown":
        pass
    else:
        identity_reasons.append("writer_cleanup_unsealed_artifact")

    expected_commit = final_commit if seal_event else task.get("workspace", {}).get("base_commit")
    if mode_name == "remove_exact":
        if mode.get("expected_branch_commit") != expected_commit:
            identity_reasons.append("writer_cleanup_expected_commit")
        if "unknown" in {ownership_state, artifact_state, scope_state}:
            identity_reasons.append("writer_cleanup_remove_without_authority")
    elif mode_name == "retain_for_recovery":
        if not isinstance(mode.get("uncertainty_code"), str):
            identity_reasons.append("writer_cleanup_retain_uncertainty")
    else:
        identity_reasons.append("writer_cleanup_mode_shape")

    expected_phase, expected_reason = expected_writer_cleanup_context(
        root_events
    )
    if (
        plan.get("phase") != expected_phase
        or plan.get("reason_code") != expected_reason
    ):
        identity_reasons.append("writer_cleanup_lifecycle_context")

    status = result.get("status")
    settled = False
    retained = False
    metadata_uncertain = False
    if status == "removed":
        resources = [result.get("worktree"), result.get("branch")]
        if not all(value in {"removed", "already_absent"} for value in resources) or resources == [
            "already_absent",
            "already_absent",
        ]:
            identity_reasons.append("writer_cleanup_removed_shape")
        else:
            settled = True
    elif status == "already_absent":
        settled = True
    elif status == "retained":
        retained = True
        resources = [result.get("worktree"), result.get("branch")]
        metadata_uncertain = result.get("metadata") == "uncertain"
        if (
            not all(
                value in {"removed", "already_absent", "retained", "unknown"}
                for value in resources
            )
            or result.get("metadata") not in {"clear", "uncertain"}
            or not isinstance(result.get("uncertainty_code"), str)
            or (
                not metadata_uncertain
                and not any(value in {"retained", "unknown"} for value in resources)
            )
        ):
            identity_reasons.append("writer_cleanup_retained_shape")
    else:
        identity_reasons.append("writer_cleanup_status_shape")

    cleanup_prepared = stored_values(root_events, "agent_cleanup_prepared")
    cleanup_committed = stored_values(root_events, "agent_cleanup_committed")
    if (
        len(cleanup_prepared) == 1
        and len(cleanup_committed) == 1
        and not (
            isinstance(cleanup_prepared[0].get("sequence"), int)
            and isinstance(cleanup_committed[0].get("sequence"), int)
            and cleanup_prepared[0]["sequence"]
            < cleanup_committed[0]["sequence"]
        )
    ):
        identity_reasons.append("writer_cleanup_event_order")

    if mode_name == "retain_for_recovery":
        if status != "retained" or len(set(uncertainty_codes)) != 1:
            identity_reasons.append("writer_cleanup_retention_mismatch")
    terminal_events = event_values(root_events, "terminal")
    terminal = (
        terminal_events[0].get("outcome", {}).get("terminal", {})
        if len(terminal_events) == 1
        else {}
    )
    if retained:
        ambiguity = terminal.get("ambiguity", {}) if isinstance(terminal, dict) else {}
        if (
            terminal.get("state") != "recovery_required"
            or ambiguity.get("phase") != "child_run"
            or ambiguity.get("action_id")
            != f"agent-cleanup:{canonical_task_id}"
            or ambiguity.get("message") != result.get("uncertainty_code")
        ):
            identity_reasons.append("writer_cleanup_terminal_mismatch")

    return {
        "identity_valid": not identity_reasons,
        "identity_reasons": list(dict.fromkeys(identity_reasons)),
        "phase": plan.get("phase"),
        "reason_code": plan.get("reason_code"),
        "ownership_state": ownership_state,
        "artifact_state": artifact_state,
        "scope": {
            "state": scope_state,
            "workspace_revision": scope.get("workspace_revision"),
            "changed_count": scope.get("changed_count"),
            "in_scope_count": scope.get("in_scope_count"),
            "out_of_scope_count": scope.get("out_of_scope_count"),
            "path_set_sha256": scope.get("path_set_sha256"),
            "has_uncertainty": isinstance(scope.get("uncertainty_code"), str),
        },
        "mode": mode_name,
        "status": status,
        "worktree": result.get("worktree"),
        "branch": result.get("branch"),
        "metadata": result.get("metadata", "clear" if settled else None),
        "settled": settled,
        "retained": retained,
        "metadata_uncertain": metadata_uncertain,
        "uncertainty_code_sha256": (
            canonical_hash(result.get("uncertainty_code"))
            if isinstance(result.get("uncertainty_code"), str)
            else None
        ),
    }


def writer_lifecycle_summary(
    task_id: str,
    root_events: list[dict[str, Any]],
    child_events: list[dict[str, Any]],
    base_commit: str,
    root_receipt: dict[str, Any],
    root_workspace: Path,
    managed_worktree_root: Path,
) -> dict[str, Any]:
    counts = {
        name: len(event_values(root_events, name))
        for name in LIFECYCLE_KINDS
    }
    prepared = event_values(root_events, "agent_task_prepared")
    task = prepared[0].get("task", {}) if len(prepared) == 1 else {}
    workspace = task.get("workspace", {}) if isinstance(task, dict) else {}
    seal = event_values(root_events, "agent_seal_committed")
    integration = event_values(root_events, "agent_integration_committed")
    failures = event_values(root_events, "agent_integration_failed")
    agent_calls = [
        event
        for event in event_values(root_events, "tool_prepared")
        if event.get("invocation", {}).get("name") == "agent"
    ]
    call_arguments = (
        agent_calls[0]
        .get("invocation", {})
        .get("arguments", {})
        .get("parsed", {})
        if len(agent_calls) == 1
        else {}
    )
    expected_arguments = expected_writer_arguments(task_id)
    expected_keys = {*expected_arguments, "prompt"}
    arguments_valid = (
        isinstance(call_arguments, dict)
        and isinstance(call_arguments.get("prompt"), str)
        and MANIFEST["tasks"][task_id]["objective"]
        in call_arguments["prompt"]
        and set(call_arguments) == expected_keys
        and all(
            call_arguments.get(key) == value
            for key, value in expected_arguments.items()
        )
    )
    root_created = run_created(root_events)
    child_created = run_created(child_events)
    worktree_path = workspace.get("worktree_path")
    worktree_managed = False
    if isinstance(worktree_path, str):
        try:
            Path(worktree_path).resolve().relative_to(
                managed_worktree_root.resolve()
            )
            worktree_managed = True
        except (OSError, ValueError):
            worktree_managed = False
    assignment_valid = (
        len(prepared) == 1
        and workspace.get("access") == "isolated_write"
        and workspace.get("root_workspace") == str(root_workspace.resolve())
        and workspace.get("base_commit") == base_commit
        and workspace.get("allowed_paths")
        == MANIFEST["tasks"][task_id]["allowed_paths"]
        and worktree_path != workspace.get("root_workspace")
        and worktree_managed
        and isinstance(workspace.get("branch"), str)
        and workspace.get("branch", "").startswith("codewhale/writer/")
        and task.get("root_run_id") == root_created.get("run_id")
        and task.get("parent_run_id") == root_created.get("run_id")
        and child_created.get("run_id") == task.get("child_run_id")
        and child_created.get("parent_run_id") == root_created.get("run_id")
        and child_created.get("agent_task") == task
    )
    seal_event = seal[0] if len(seal) == 1 else {}
    integration_event = integration[0] if len(integration) == 1 else {}
    any_lifecycle = any(counts.values())
    cleanup_summary = {
        "identity_valid": True,
        "identity_reasons": [],
        "phase": None,
        "reason_code": None,
        "ownership_state": None,
        "artifact_state": None,
        "scope": {"state": None},
        "mode": None,
        "status": None,
        "worktree": None,
        "branch": None,
        "metadata": None,
        "settled": False,
        "retained": False,
        "metadata_uncertain": False,
        "uncertainty_code_sha256": None,
    }
    identity_reasons: list[str] = []
    if len(prepared) == 1:
        canonical_task_id = task.get("task_id")
        cleanup_summary = writer_cleanup_summary(
            canonical_task_id if isinstance(canonical_task_id, str) else "",
            root_events,
            task,
            seal_event,
            integration_event,
        )
        identity_reasons.extend(cleanup_summary["identity_reasons"])
        for name in LIFECYCLE_KINDS:
            if name in {"agent_task_prepared", "child_finished"}:
                continue
            for event in event_values(root_events, name):
                if event.get("task_id") != canonical_task_id:
                    identity_reasons.append("writer_lifecycle_task_identity")
        if counts["child_finished"] == 1 and (
            event_values(root_events, "child_finished")[0].get("call_id")
            != task.get("call_id")
        ):
            identity_reasons.append("writer_lifecycle_call_identity")
        child_started_events = event_values(root_events, "child_started")
        if len(child_started_events) == 1 and (
            child_started_events[0].get("child_run_id")
            != task.get("child_run_id")
            or child_started_events[0].get("call_id") != task.get("call_id")
        ):
            identity_reasons.append("writer_lifecycle_child_identity")
        for name, count in counts.items():
            if count > 1:
                identity_reasons.append(f"writer_lifecycle_duplicate_{name}")
    elif any_lifecycle:
        identity_reasons.append("writer_task_cardinality")
    changed_files = seal_event.get("changed_files", [])
    latest_receipt = False
    integrated_state = integration_event.get("root_workspace_state_after")
    receipt_state = root_receipt.get("workspace_state")
    if isinstance(integrated_state, dict) and isinstance(receipt_state, dict):
        before_generation = integrated_state.get("generation")
        latest_receipt = (
            isinstance(before_generation, int)
            and receipt_state.get("generation") == before_generation + 1
            and receipt_state.get("revision") == integrated_state.get("revision")
        )
    child_terminal = typed_event_terminal_state(child_events)
    child_receipt = completion_receipt_summary(
        task_id, child_events, child_terminal
    )
    seal_prepared = event_values(root_events, "agent_seal_prepared")
    worktree_verified = (
        child_terminal == "completed"
        and child_receipt["valid"]
        and len(seal_prepared) == 1
        and seal_prepared[0].get("writer_workspace_state_before")
        == child_receipt.get("workspace_state")
    )
    successful_chain = (
        len(agent_calls) == 1
        and arguments_valid
        and assignment_valid
        and not failures
        and all(
            counts[name] == 1
            for name in (
                "agent_workspace_created",
                "child_started",
                "agent_seal_prepared",
                "agent_seal_committed",
                "agent_result_collected",
                "agent_integration_prepared",
                "agent_integration_started",
                "agent_integration_committed",
                "child_finished",
                "agent_cleanup_prepared",
                "agent_cleanup_committed",
            )
        )
        and cleanup_summary["identity_valid"]
        and cleanup_summary["phase"] == "post_integration"
        and cleanup_summary["reason_code"] == "writer_integrated"
        and cleanup_summary["scope"].get("state") == "known"
        and cleanup_summary["scope"].get("out_of_scope_count") == 0
        and cleanup_summary["settled"]
        and changed_files == MANIFEST["tasks"][task_id]["expected_changed_files"]
        and latest_receipt
        and worktree_verified
    )
    return {
        "agent_tool_calls": len(agent_calls),
        "arguments_valid": arguments_valid,
        "assignment_valid": assignment_valid,
        "event_counts": counts,
        "integration_failures": len(failures),
        "identity_valid": not identity_reasons,
        "identity_reasons": list(dict.fromkeys(identity_reasons)),
        "successful_chain": successful_chain,
        "latest_root_receipt": latest_receipt,
        "worktree_verifier_valid": worktree_verified,
        "child_terminal_state": child_terminal,
        "child_receipt_sha256": child_receipt["receipt_sha256"],
        "base_commit": workspace.get("base_commit"),
        "final_commit": seal_event.get("final_commit"),
        "root_head_commit": integration_event.get("root_head_commit"),
        "diff_sha256": seal_event.get("diff_sha256"),
        "changed_files": changed_files,
        "allowed_paths": workspace.get("allowed_paths"),
        "worktree_identity_sha256": (
            canonical_hash(workspace.get("worktree_path"))
            if isinstance(workspace.get("worktree_path"), str)
            else None
        ),
        "branch_identity_sha256": (
            canonical_hash(workspace.get("branch"))
            if isinstance(workspace.get("branch"), str)
            else None
        ),
        "cleanup": cleanup_summary,
    }


def git_evidence(
    workspace: Path,
    base_commit: str,
    state_root: Path,
    treatment: str,
    deadline: float,
) -> dict[str, Any]:
    head = run_git(
        workspace, "rev-parse", "HEAD", deadline=deadline
    ).stdout.strip()
    status = run_git(
        workspace,
        "status",
        "--porcelain=v1",
        "--untracked-files=all",
        deadline=deadline,
    ).stdout.splitlines()
    changed = run_git(
        workspace,
        "diff",
        "--name-only",
        "--no-renames",
        base_commit,
        deadline=deadline,
    ).stdout.splitlines()
    untracked = run_git(
        workspace,
        "ls-files",
        "--others",
        "--exclude-standard",
        deadline=deadline,
    ).stdout.splitlines()
    changed_files = sorted(set(changed + untracked))
    worktrees = [
        line.removeprefix("worktree ")
        for line in run_git(
            workspace,
            "worktree",
            "list",
            "--porcelain",
            deadline=deadline,
        )
        .stdout.splitlines()
        if line.startswith("worktree ")
    ]
    writer_refs = run_git(
        workspace,
        "for-each-ref",
        "--format=%(refname)",
        "refs/heads/codewhale/writer/",
        deadline=deadline,
    ).stdout.splitlines()
    managed = state_root / "codewhale" / "worktrees"
    managed_entries = (
        [entry.name for entry in managed.iterdir()]
        if managed.is_dir()
        else []
    )
    leak_free = (
        worktrees == [str(workspace.resolve())]
        and not writer_refs
        and not managed_entries
    )
    commit_count = int(
        run_git(
            workspace,
            "rev-list",
            "--count",
            f"{base_commit}..{head}",
            deadline=deadline,
        )
        .stdout.strip()
    )
    expected_clean = treatment == "writer"
    return {
        "base_commit": base_commit,
        "head_commit": head,
        "commit_count": commit_count,
        "changed_files": changed_files,
        "status_count": len(status),
        "root_clean": not status,
        "root_clean_matches_treatment": (not status) == expected_clean,
        "registered_worktrees": len(worktrees),
        "writer_refs": len(writer_refs),
        "managed_worktree_entries": len(managed_entries),
        "leak_free": leak_free,
        "final_tree_sha256": canonical_hash(snapshot_tree(workspace)),
    }


def prefixed_sha256(value: Any) -> bool:
    return (
        isinstance(value, str)
        and value.startswith("sha256:")
        and raw_sha256(value.removeprefix("sha256:"))
    )


def admission_audit(
    task_id: str,
    treatment: str,
    root_events: list[dict[str, Any]],
    child_events: list[dict[str, Any]],
    workspace: Path,
) -> dict[str, Any]:
    reasons: list[str] = []
    root = run_created(root_events)
    child = run_created(child_events)
    expected = start_command(task_id, treatment, workspace, "freeze-request")[
        "command"
    ]
    environment = root.get("environment", {})
    actor = root.get("actor", {})
    if actor != {"kind": "root", "depth": 0}:
        reasons.append("root_actor_identity")
    if root.get("parent_run_id") is not None:
        reasons.append("root_parent_identity")
    if root.get("model") != expected["model"]:
        reasons.append("root_model_identity")
    if root.get("reasoning_effort") != expected["reasoning_effort"]:
        reasons.append("root_reasoning_identity")
    if root.get("max_output_tokens") != expected["max_output_tokens"]:
        reasons.append("root_output_limit_identity")
    if root.get("streaming") != expected["streaming"]:
        reasons.append("root_streaming_identity")
    if root.get("tool_policy") != expected["tool_policy"]:
        reasons.append("root_tool_policy_identity")
    persisted_limits = root.get("limits", {})
    if any(
        persisted_limits.get(name) != value
        for name, value in expected["limits"].items()
    ):
        reasons.append("root_limits_identity")
    expected_controls = expected["controls"]
    expected_environment = {
        "workspace": str(workspace.resolve()),
        "provider": "deepseek",
        "write_execution_mode": expected_controls["write_execution_mode"],
        "auto_approve": expected_controls["auto_approve"],
        "trust_mode": expected_controls["trust_mode"],
        "allow_sandbox_elevation": expected_controls[
            "allow_sandbox_elevation"
        ],
        "interactive": expected_controls["interactive"],
        "sandbox": expected_controls["sandbox"],
    }
    if any(environment.get(name) != value for name, value in expected_environment.items()):
        reasons.append("root_environment_identity")
    if not prefixed_sha256(environment.get("execution_fingerprint_sha256")):
        reasons.append("root_execution_fingerprint")

    if treatment == "single":
        if child_events:
            reasons.append("single_unexpected_child")
    elif child_events:
        child_environment = child.get("environment", {})
        if child.get("actor") != {"kind": "child", "depth": 1}:
            reasons.append("writer_child_actor_identity")
        if child.get("parent_run_id") != root.get("run_id"):
            reasons.append("writer_child_parent_identity")
        if child_environment.get("write_execution_mode") != "isolated_writer":
            reasons.append("writer_child_mode_identity")
        if child_environment.get("execution_fingerprint_sha256") is not None:
            reasons.append("writer_child_execution_fingerprint")
        if child.get("model") != root.get("model"):
            reasons.append("writer_child_model_identity")
    return {
        "valid": not reasons,
        "reasons": list(dict.fromkeys(reasons)),
        "write_execution_mode": environment.get("write_execution_mode"),
        "root_execution_fingerprint_present": prefixed_sha256(
            environment.get("execution_fingerprint_sha256")
        ),
        "child_execution_fingerprint_absent": (
            not child_events
            or child.get("environment", {}).get("execution_fingerprint_sha256")
            is None
        ),
    }


def treatment_audit(
    task_id: str,
    treatment: str,
    root_events: list[dict[str, Any]],
    child_events: list[dict[str, Any]],
    writer: dict[str, Any],
    git: dict[str, Any],
) -> dict[str, Any]:
    root_tools = tool_names(root_events)
    root_direct_writes = [
        event.get("invocation", {}).get("name")
        for event in event_values(root_events, "tool_prepared")
        if event.get("workspace_access") == "may_write"
        and event.get("invocation", {}).get("name") != "agent"
    ]
    agent_count = root_tools.count("agent")
    child_count = len(event_values(root_events, "child_started"))
    root_catalogs = catalog_summary(root_events, "root")
    child_catalogs = catalog_summary(child_events, "child")
    empty_hash = frozen_catalog_hash(task_id, "terminal_empty")
    root_key = "single_root" if treatment == "single" else "writer_root"
    root_catalog_audit = catalog_identity_audit(
        root_catalogs,
        frozen_catalog_hash(task_id, root_key),
        empty_hash,
    )
    child_catalog_audit = {
        "valid": True,
        "reasons": [],
        "logical_requests": 0,
        "terminal_empty_logical_requests": 0,
        "observed": False,
    }
    root_created = run_created(root_events)
    child_created = run_created(child_events)
    root_environment_valid = (
        root_created.get("environment", {}).get("tool_catalog_sha256")
        == frozen_catalog_hash(task_id, root_key)
    )
    child_environment_valid = treatment == "single"
    if treatment == "single":
        catalog_valid = root_catalog_audit["valid"] and root_environment_valid
        contract_valid = (
            catalog_valid
            and agent_count == 0
            and child_count == 0
            and not child_events
            and set(git["changed_files"])
            == set(MANIFEST["tasks"][task_id]["expected_changed_files"])
        )
    else:
        child_environment_valid = not child_events
        if child_events:
            child_catalog_audit = catalog_identity_audit(
                child_catalogs,
                frozen_catalog_hash(task_id, "writer_child"),
                empty_hash,
            )
            child_catalog_audit["observed"] = True
            child_environment_valid = (
                child_created.get("environment", {}).get(
                    "tool_catalog_sha256"
                )
                == frozen_catalog_hash(task_id, "writer_child")
            )
        catalog_valid = (
            root_catalog_audit["valid"]
            and child_catalog_audit["valid"]
            and root_environment_valid
            and child_environment_valid
        )
        contract_valid = (
            catalog_valid
            and agent_count == 1
            and child_count == 1
            and not root_direct_writes
            and writer["successful_chain"]
            and git["leak_free"]
            and git["root_clean"]
            and git["commit_count"] == 1
            and git["head_commit"] == writer["final_commit"]
            and git["head_commit"] == writer["root_head_commit"]
        )
    return {
        "valid": contract_valid,
        "catalog_valid": catalog_valid,
        "catalog_identity_reasons": list(
            dict.fromkeys(
                [
                    *root_catalog_audit["reasons"],
                    *child_catalog_audit["reasons"],
                    *([] if root_environment_valid else ["root_catalog_environment"]),
                    *([] if child_environment_valid else ["child_catalog_environment"]),
                ]
            )
        ),
        "root_catalog_requests": root_catalog_audit["logical_requests"],
        "child_catalog_requests": child_catalog_audit["logical_requests"],
        "root_terminal_empty_catalog_requests": root_catalog_audit[
            "terminal_empty_logical_requests"
        ],
        "child_terminal_empty_catalog_requests": child_catalog_audit[
            "terminal_empty_logical_requests"
        ],
        "root_direct_write_violation": bool(root_direct_writes),
        "root_direct_write_tools": root_direct_writes,
        "agent_tool_calls": agent_count,
        "child_started": child_count,
    }


def terminal_state(run: dict[str, Any]) -> str | None:
    terminal = run.get("terminal")
    return terminal.get("state") if isinstance(terminal, dict) else None


def typed_terminal_summary(run: dict[str, Any]) -> dict[str, Any]:
    terminal = run.get("terminal")
    if not isinstance(terminal, dict):
        return {"state": None, "reason_sha256": None}
    state = terminal.get("state")
    reason = terminal.get("reason")
    reason_code = None
    reason_value: Any = reason
    if state == "blocked" and isinstance(reason, str):
        reason_code = reason.split("：", 1)[0]
    elif state == "failed" and isinstance(terminal.get("failure"), dict):
        reason_value = terminal["failure"]
        reason_code = terminal["failure"].get("kind")
    elif state == "recovery_required" and isinstance(
        terminal.get("ambiguity"), dict
    ):
        reason_value = terminal["ambiguity"]
        reason_code = terminal["ambiguity"].get("action_id")
    return {
        "state": state,
        "reason_sha256": (
            canonical_hash(reason_value) if reason_value is not None else None
        ),
        "reason_code": reason_code,
    }


def result_redacted(value: Any, key: str) -> None:
    encoded = canonical_bytes(value)
    if key and key.encode() in encoded:
        raise EvaluationError("result_contains_key")
    for prefix in (
        b"/private/tmp/codewhale-m6b-arm-",
        b"/tmp/codewhale-m6b-arm-",
        b"/var/folders/",
    ):
        if prefix in encoded:
            raise EvaluationError("result_contains_temporary_path")


def binary_identity(binary: Path, revision: str) -> dict[str, Any]:
    source = binary.expanduser().resolve()
    if (
        not source.is_file()
        or not os.access(source, os.X_OK)
        or len(revision) != 40
        or any(character not in "0123456789abcdef" for character in revision)
    ):
        raise EvaluationError("candidate_identity_invalid")
    with tempfile.TemporaryDirectory(prefix="codewhale-m6b-probe-") as raw:
        root = Path(raw)
        workspace, _, _ = initialize_workspace("t1", root)
        environment = {
            **CANARY.safe_env(),
            "HOME": str(root / "home"),
            "CODEWHALE_HOME": str(root / "state"),
            "XDG_CONFIG_HOME": str(root / "xdg"),
        }
        for key in ("HOME", "CODEWHALE_HOME", "XDG_CONFIG_HOME"):
            Path(environment[key]).mkdir(parents=True, exist_ok=True)
        probed = CANARY.probe_binary(source, workspace, environment, revision)
    digest = file_hash(source)
    return {
        "revision": revision,
        "sha256": digest,
        "version": f"codewhale {probed['version']} ({probed['revision_prefix']})",
        "build_revision_bound": probed["revision_prefix"] == revision[:12],
    }


def validate_repository_identity(revision: str) -> None:
    head = run_git(ROOT, "rev-parse", "HEAD").stdout.strip()
    status = run_git(
        ROOT, "status", "--porcelain=v1", "--untracked-files=all"
    ).stdout.splitlines()
    reasons = []
    if head != revision:
        reasons.append("candidate_revision_not_repository_head")
    if status:
        reasons.append("repository_worktree_not_clean")
    if reasons:
        raise EvaluationError(
            "repository_identity_invalid",
            {
                "reasons": reasons,
                "head": head,
                "candidate_revision": revision,
                "dirty_entry_count": len(status),
            },
        )


def validate_live_identity(
    identity: dict[str, Any],
    binary: Path,
    revision: str,
) -> None:
    validate_repository_identity(revision)
    validate_frozen_hashes()
    candidate = identity.get("candidate", {})
    try:
        binary_digest = file_hash(binary.expanduser().resolve())
    except OSError as error:
        raise EvaluationError("candidate_binary_changed") from error
    if not isinstance(candidate, dict) or binary_digest != candidate.get("sha256"):
        raise EvaluationError("candidate_binary_changed")
    if freeze_identity(candidate) != identity:
        raise EvaluationError("frozen_evaluation_identity_changed")


def query(kind: str, run_id: str, request_id: str) -> dict[str, Any]:
    return CANARY.query(kind, run_id, request_id)


def cancel_was_accepted(response: dict[str, Any], run_id: str) -> bool:
    return (
        response.get("kind") == "accepted"
        and response.get("run_id") == run_id
    )


def cancel_requires_canonical_get(error: CANARY.Failure) -> bool:
    return error.code in {
        "run_api_run_not_active",
        "run_api_run_terminal",
    }


def cancel_conflict_failure(
    error: CANARY.Failure,
    run: dict[str, Any],
) -> str | None:
    if run.get("terminal") is not None:
        return None
    return (
        "run_inactive_without_terminal"
        if error.code == "run_api_run_not_active"
        else "run_terminal_without_terminal"
    )


def collect_events(
    client: Any,
    run_id: str,
    request_id: str,
    deadline: float,
) -> list[dict[str, Any]]:
    return CANARY.events(
        client,
        run_id,
        request_id,
        timeout_seconds=remaining_stdio_timeout(deadline),
    )


def remaining_stdio_timeout(deadline: float) -> float:
    remaining = deadline - time.monotonic()
    if remaining <= 0:
        raise EvaluationError("arm_harness_deadline")
    return min(RESOURCES["stdio_poll_timeout_seconds"], remaining)


def failure_arm_record(
    task_id: str,
    treatment: str,
    pair_index: int,
    attempt_index: int,
    order: list[str],
    arm_position: int,
    code: str,
    api_exposure: str,
    duration_ms: int,
) -> dict[str, Any]:
    billing_unknown = api_exposure != "none"
    return {
        "task_id": task_id,
        "treatment": treatment,
        "pair_index": pair_index,
        "attempt_index": attempt_index,
        "pair_order": order,
        "arm_position": arm_position,
        "product_outcome_observed": False,
        "measurement_valid": False,
        "measurement_invalid_reasons": [code],
        "mixed_product_failure_and_measurement_gap": False,
        "verified_success": False,
        "false_success": False,
        "task_success_before_measurement": None,
        "terminal": {"state": None, "reason_code": None, "reason_sha256": None},
        "requests": {
            "root": {
                "started": None if billing_unknown else 0,
                "completed": None if billing_unknown else 0,
                "in_flight": None if billing_unknown else 0,
                "retries": None if billing_unknown else 0,
            },
            "child": {
                "started": None if billing_unknown else 0,
                "completed": None if billing_unknown else 0,
                "in_flight": None if billing_unknown else 0,
                "retries": None if billing_unknown else 0,
            },
            "transport_retries": None if billing_unknown else 0,
            "runtime_retries": None if billing_unknown else 0,
            "usage": usage_zero(),
            "cost_nanousd": 0,
            "cost_nanocny": 0,
            "billing_unknown": billing_unknown,
            "known_cost_is_lower_bound": billing_unknown,
            "request_count_unknown": billing_unknown,
        },
        "accounting_provenance": {
            "source": "none",
            "terminal_sequence": None,
            "run_last_sequence": None,
            "terminal_count": 0,
            "terminal_is_last": False,
            "terminal_matches_run_view": False,
            "terminal_accounting_matches_run_view": False,
        },
        "child_accounting_provenance": {
            "source": "none",
            "terminal_sequence": None,
            "run_last_sequence": None,
            "terminal_count": 0,
            "terminal_is_last": False,
            "terminal_matches_run_view": False,
            "terminal_accounting_matches_run_view": False,
        },
        "api_exposure": api_exposure,
        "wall_time_ms": duration_ms,
        "failure_code": code,
        "hard_safety_violation": code in SECRET_BOUNDARY_FAILURE_CODES,
    }


def pair_measurement_classification(
    arms: list[dict[str, Any]],
) -> dict[str, bool]:
    measurement_valid = bool(arms) and all(
        arm.get("measurement_valid") is True for arm in arms
    )
    has_gap = not measurement_valid
    has_product_failure = any(
        arm.get("product_outcome_observed") is True
        and arm.get("task_success_before_measurement") is False
        for arm in arms
    )
    has_unobserved_execution = any(
        arm.get("measurement_valid") is not True
        and arm.get("api_exposure") != "none"
        and arm.get("product_outcome_observed") is not True
        for arm in arms
    )
    mixed = has_gap and has_product_failure
    return {
        "measurement_valid": measurement_valid,
        "mixed_product_failure_and_measurement_gap": mixed,
        "unobserved_model_execution": has_unobserved_execution,
    }


def arm_hard_stop_reasons(arm: dict[str, Any]) -> list[str]:
    reasons = []
    if arm.get("measurement_valid") is not True:
        reasons.extend(arm.get("measurement_invalid_reasons", []))
    reasons.extend(arm_hard_mechanism_reasons(arm))
    return list(dict.fromkeys(str(reason) for reason in reasons))


def arm_hard_mechanism_reasons(arm: dict[str, Any]) -> list[str]:
    reasons = []
    if arm.get("hard_safety_violation") is True:
        reasons.append("harness_safety_violation")
    if arm.get("measurement_valid") is True:
        if arm.get("false_success") is True:
            reasons.append("false_success")
        if arm.get("treatment") == "writer":
            reasons.extend(
                f"writer_{reason}" for reason in writer_safety_reasons(arm)
            )
    return list(dict.fromkeys(reasons))


def execute_arm(
    binary: Path,
    binary_sha256: str,
    revision: str,
    key: str,
    task_id: str,
    treatment: str,
    pair_index: int,
    attempt_index: int,
    order: list[str],
    arm_position: int,
    execution_state: dict[str, Any],
) -> dict[str, Any]:
    arm_started = time.monotonic()
    deadline = arm_started + RESOURCES["harness_wall_time_seconds"]
    arm_started_at_utc = time.strftime(
        "%Y-%m-%dT%H:%M:%SZ", time.gmtime()
    )
    evaluation_id = str(uuid.uuid4())
    execution_state["api_exposure"] = "none"
    execution_state["harness_cancel_sent"] = False
    execution_state["harness_cancel_raced"] = False
    secret = key.encode()
    with tempfile.TemporaryDirectory(prefix="codewhale-m6b-arm-") as raw:
        ephemeral = Path(raw)
        workspace, base_commit, initial_tree = initialize_workspace(
            task_id, ephemeral, deadline
        )
        if file_hash(binary) != binary_sha256:
            raise EvaluationError("candidate_binary_changed")
        executable = ephemeral / "codewhale"
        shutil.copy2(binary, executable)
        executable.chmod(0o700)
        if file_hash(executable) != binary_sha256:
            raise EvaluationError("candidate_copy_changed")

        state_root = ephemeral / "state"
        home = state_root / "home"
        codewhale_home = state_root / "codewhale"
        xdg = state_root / "xdg"
        for directory in (home, codewhale_home, xdg):
            directory.mkdir(parents=True)
        environment = {
            **CANARY.safe_env(),
            "HOME": str(home),
            "CODEWHALE_HOME": str(codewhale_home),
            "XDG_CONFIG_HOME": str(xdg),
            "DEEPSEEK_API_KEY": key,
        }
        if any(name in environment for name in CANARY.NETWORK_OVERRIDE_ENV):
            raise EvaluationError("network_override_inherited")

        stderr_path = state_root / "app-server.stderr"
        try:
            with stderr_path.open("wb") as stderr_stream:
                process = subprocess.Popen(
                    [
                        str(executable),
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
        except OSError as error:
            raise EvaluationError("app_server_launch_failed") from error
        environment["DEEPSEEK_API_KEY"] = ""
        client = None
        run: dict[str, Any] = {}
        root_events: list[dict[str, Any]] = []
        child_events: list[dict[str, Any]] = []
        child_run: dict[str, Any] = {}
        child_ids: list[str] = []
        child_lookup_reasons: list[str] = []
        harness_cancel_sent = False
        harness_cancel_attempted = False
        harness_cancel_raced = False
        pending_error: Exception | None = None
        try:
            client = CANARY.Stdio(process, secret)
            envelope = start_command(
                task_id,
                treatment,
                workspace,
                f"m6b-start-{evaluation_id}",
            )
            if secret in canonical_bytes(envelope):
                raise EvaluationError("key_in_protocol")
            execution_state["api_exposure"] = "possible"
            response = client.call(
                envelope,
                timeout_seconds=remaining_stdio_timeout(deadline),
            )
            if response.get("kind") != "run":
                raise EvaluationError("start_run_missing")
            runtime_started = time.monotonic()
            run = response.get("run", {})
            run_id = run.get("run_id")
            if not isinstance(run_id, str):
                raise EvaluationError("root_run_id_missing")
            runtime_deadline = (
                runtime_started + RESOURCES["runtime_wall_time_seconds"]
            )
            cancel_deadline = (
                runtime_deadline
                + RESOURCES["runtime_cancel_grace_seconds"]
            )
            poll = 0
            while run.get("terminal") is None:
                poll += 1
                now = time.monotonic()
                if now >= deadline:
                    raise EvaluationError("arm_harness_deadline")
                if process.poll() is not None:
                    raise EvaluationError("app_server_exited")
                if (
                    not harness_cancel_attempted
                    and now >= cancel_deadline
                ):
                    harness_cancel_attempted = True
                    try:
                        response = client.call(
                            query(
                                "cancel",
                                run_id,
                                f"m6b-cancel-{evaluation_id}",
                            ),
                            timeout_seconds=remaining_stdio_timeout(deadline),
                        )
                    except CANARY.Failure as error:
                        if not cancel_requires_canonical_get(error):
                            raise
                        response = client.call(
                            query(
                                "get",
                                run_id,
                                f"m6b-cancel-conflict-get-{evaluation_id}",
                            ),
                            timeout_seconds=remaining_stdio_timeout(deadline),
                        )
                        if response.get("kind") != "run":
                            raise EvaluationError("run_view_missing")
                        run = response.get("run", {})
                        execution_state["api_exposure"] = "accounted"
                        execution_state["accounting"] = (
                            provisional_accounting_summary(run)
                        )
                        execution_state["accounting_provenance"] = (
                            provisional_accounting_provenance(run)
                        )
                        if code := cancel_conflict_failure(error, run):
                            raise EvaluationError(code)
                        harness_cancel_raced = True
                        execution_state["harness_cancel_raced"] = True
                        continue
                    if not cancel_was_accepted(response, run_id):
                        raise EvaluationError("cancel_not_accepted")
                    harness_cancel_sent = True
                    execution_state["harness_cancel_sent"] = True
                    continue
                time.sleep(0.2)
                response = client.call(
                    query("get", run_id, f"m6b-get-{evaluation_id}-{poll}"),
                    timeout_seconds=remaining_stdio_timeout(deadline),
                )
                if response.get("kind") != "run":
                    raise EvaluationError("run_view_missing")
                run = response.get("run", {})
                execution_state["api_exposure"] = "accounted"
                execution_state["accounting"] = provisional_accounting_summary(run)
                execution_state["accounting_provenance"] = (
                    provisional_accounting_provenance(run)
                )

            execution_state["api_exposure"] = "accounted"
            execution_state["accounting"] = provisional_accounting_summary(run)
            execution_state["accounting_provenance"] = (
                provisional_accounting_provenance(run)
            )
            root_events = collect_events(
                client,
                run_id,
                f"m6b-root-events-{evaluation_id}",
                deadline,
            )
            child_ids = [
                event.get("task", {}).get("child_run_id")
                for event in event_values(root_events, "agent_task_prepared")
                if isinstance(event.get("task", {}).get("child_run_id"), str)
            ]
            if len(child_ids) == 1:
                response = client.call(
                    query(
                        "get",
                        child_ids[0],
                        f"m6b-child-get-{evaluation_id}",
                    ),
                    timeout_seconds=remaining_stdio_timeout(deadline),
                )
                if response.get("kind") == "run":
                    value = response.get("run")
                    if isinstance(value, dict):
                        child_run = value
                        child_events = collect_events(
                            client,
                            child_ids[0],
                            f"m6b-child-events-{evaluation_id}",
                            deadline,
                        )
                    else:
                        child_lookup_reasons.append("child_run_view_missing")
                else:
                    child_lookup_reasons.append("child_run_view_missing")
            elif len(child_ids) > 1:
                child_lookup_reasons.append("child_terminal_cardinality")
        except CANARY.Failure as error:
            pending_error = EvaluationError(error.code)
        except Exception as error:
            pending_error = error
        finally:
            cleanup_error: Exception | None = None
            if client is not None:
                try:
                    client.close()
                except Exception as error:
                    cleanup_error = error
            try:
                CANARY.stop(
                    process,
                    timeout_seconds=max(0.0, deadline - time.monotonic()),
                )
            except Exception as error:
                cleanup_error = cleanup_error or error
            pending_error = pending_error or cleanup_error

        secret_failures: list[str] = []
        scan_incomplete = False
        try:
            stderr = stderr_path.read_bytes() if stderr_path.is_file() else b""
            if secret in stderr:
                secret_failures.append("key_in_stderr")
        except Exception:
            scan_incomplete = True
        try:
            if CANARY.tree_contains(workspace, secret):
                secret_failures.append("key_in_fixture")
        except Exception:
            scan_incomplete = True
        try:
            if CANARY.tree_contains(state_root, secret):
                secret_failures.append("key_in_state")
        except Exception:
            scan_incomplete = True
        if secret_failures:
            raise EvaluationError(secret_failures[0])
        if scan_incomplete:
            raise EvaluationError("secret_scan_incomplete")
        if pending_error is not None:
            if isinstance(pending_error, EvaluationError):
                raise pending_error
            raise EvaluationError("harness_internal_error") from pending_error

        state_schema = state_schema_summary(codewhale_home)
        terminal = typed_terminal_summary(run)
        accounting, accounting_provenance, provenance_reasons = (
            canonical_terminal_accounting(run, root_events)
        )
        execution_state["api_exposure"] = "accounted"
        execution_state["accounting"] = accounting
        execution_state["accounting_provenance"] = accounting_provenance
        accounting_is_valid, accounting_reasons = accounting_valid(accounting)
        if not accounting_is_valid:
            accounting = lower_bound_accounting_summary(accounting)
            execution_state["accounting"] = accounting
        accounting_reasons.extend(provenance_reasons)
        accounting_reasons.extend(child_lookup_reasons)
        if provenance_reasons:
            accounting_is_valid = False
        if child_lookup_reasons:
            accounting_is_valid = False
        child_accounting_provenance = provisional_accounting_provenance(
            child_run
        )
        child_accounting_provenance["source"] = "none"
        if len(child_ids) == 1:
            _, child_accounting_provenance, child_provenance_reasons = (
                canonical_terminal_accounting(child_run, child_events)
            )
            if child_provenance_reasons:
                accounting_reasons.extend(
                    f"child_{reason}" for reason in child_provenance_reasons
                )
                accounting_is_valid = False
        budget_terminal_valid = (
            nonnegative_int_or_zero(accounting["exhausted_denied"]) == 0
            or terminal["reason_code"]
            == "api_request_budget_exceeded"
        )
        if not budget_terminal_valid:
            accounting_reasons.append("budget_terminal_attribution")
            accounting_is_valid = False
        root_actor_usage = actor_usage(root_events)
        child_actor_usage = actor_usage(child_events)
        actor_total = usage_zero()
        add_usage(actor_total, root_actor_usage["usage"])
        add_usage(actor_total, child_actor_usage["usage"])
        actor_usage_shape_valid = (
            root_actor_usage["shape_valid"]
            and child_actor_usage["shape_valid"]
        )
        actor_usage_matches = (
            actor_usage_shape_valid and actor_total == accounting["usage"]
        )
        usage_response_count_matches = (
            root_actor_usage["response_count"]
            + child_actor_usage["response_count"]
            == accounting["usage_responses"]
        )
        actor_response_counts_within_requests = (
            root_actor_usage["response_count"]
            <= nonnegative_int_or_zero(accounting["root"]["completed"])
            and child_actor_usage["response_count"]
            <= nonnegative_int_or_zero(accounting["child"]["completed"])
        )
        if not actor_usage_shape_valid:
            accounting_reasons.append("actor_usage_shape")
            accounting_is_valid = False
        if not actor_usage_matches:
            accounting_reasons.append("actor_usage_mismatch")
            accounting_is_valid = False
        if not usage_response_count_matches:
            accounting_reasons.append("usage_response_count_mismatch")
            accounting_is_valid = False
        if not actor_response_counts_within_requests:
            accounting_reasons.append(
                "actor_response_count_exceeds_requests"
            )
            accounting_is_valid = False

        created = run_created(root_events)
        task_contract_valid = (
            created.get("task_contract", {}).get("definition")
            == task_definition(task_id)
            and created.get("model") == MODEL
            and created.get("reasoning_effort") == RESOURCES["reasoning_effort"]
            and created.get("max_output_tokens")
            == RESOURCES["max_output_tokens_per_request"]
            and created.get("tool_policy", {}).get("allowed") == COMMON_TOOLS
        )
        admission = admission_audit(
            task_id, treatment, root_events, child_events, workspace
        )
        root_receipt = completion_receipt_summary(
            task_id, root_events, terminal["state"]
        )
        writer = writer_lifecycle_summary(
            task_id,
            root_events,
            child_events,
            base_commit,
            root_receipt,
            workspace,
            codewhale_home / "worktrees",
        )
        git = git_evidence(
            workspace,
            base_commit,
            state_root,
            treatment,
            deadline,
        )
        exact_verifier = run_external_verifier(
            task_id, workspace, deadline
        )
        treatment_result = treatment_audit(
            task_id,
            treatment,
            root_events,
            child_events,
            writer,
            git,
        )
        expected_changed = MANIFEST["tasks"][task_id]["expected_changed_files"]
        scope_valid = git["changed_files"] == expected_changed
        path_scope_valid = set(git["changed_files"]).issubset(
            MANIFEST["tasks"][task_id]["allowed_paths"]
        )
        t3_recovery = {
            "valid": True,
            "lineage_policy": None,
            "named_verifier_reference_valid": True,
            "named_verifier_call_count": 0,
            "failed_then_write_then_host_pass": True,
            "verifier_spec_sha256": canonical_hash(verifier_spec("t3")),
        }
        if task_id == "t3":
            t3_recovery = t3_recovery_audit(
                treatment, root_events, child_events
            )

        receipt_valid = root_receipt["valid"]
        wall_time_ms = int((time.monotonic() - arm_started) * 1000)
        wall_time_valid = (
            wall_time_ms
            <= RESOURCES["harness_wall_time_seconds"] * 1000
        )
        task_success_before_measurement = (
            terminal["state"] == "completed"
            and exact_verifier["passed"]
            and task_contract_valid
            and treatment_result["valid"]
            and scope_valid
            and receipt_valid
            and t3_recovery["valid"]
            and wall_time_valid
        )
        verified_success = (
            task_success_before_measurement
            and accounting_is_valid
        )
        false_success = (
            terminal["state"] == "completed"
            and not task_success_before_measurement
        )
        product_failure = not task_success_before_measurement
        measurement_invalid_reasons = list(dict.fromkeys(accounting_reasons))
        if not root_events:
            measurement_invalid_reasons.append("root_events_missing")
        if not state_schema["valid"]:
            measurement_invalid_reasons.append("state_schema_mismatch")
        if not task_contract_valid:
            measurement_invalid_reasons.append("task_contract_identity")
        if not admission["valid"]:
            measurement_invalid_reasons.extend(admission["reasons"])
        if not treatment_result["catalog_valid"]:
            measurement_invalid_reasons.extend(
                treatment_result["catalog_identity_reasons"]
            )
        if treatment == "writer" and not writer["identity_valid"]:
            measurement_invalid_reasons.extend(writer["identity_reasons"])
        measurement_valid = not measurement_invalid_reasons
        mixed_failure_gap = product_failure and not measurement_valid
        catalogs = {
            "root": catalog_summary(root_events, "root"),
            "child": catalog_summary(child_events, "child"),
            "root_environment_sha256": created.get("environment", {}).get(
                "tool_catalog_sha256"
            ),
            "child_environment_sha256": run_created(child_events)
            .get("environment", {})
            .get("tool_catalog_sha256")
            if child_events
            else None,
        }
        event_counts = dict(
            sorted(Counter(event_kind(event) for event in root_events).items())
        )
        child_event_counts = dict(
            sorted(Counter(event_kind(event) for event in child_events).items())
        )
        record = {
            "arm_id": canonical_hash(
                [task_id, treatment, pair_index, attempt_index, evaluation_id]
            ),
            "task_id": task_id,
            "treatment": treatment,
            "pair_index": pair_index,
            "attempt_index": attempt_index,
            "pair_order": order,
            "arm_position": arm_position,
            "started_at_utc": arm_started_at_utc,
            "binary_revision": revision,
            "binary_sha256": binary_sha256,
            "model": MODEL,
            "api_surface": "standard_chat",
            "fixture_tree_sha256": initial_tree,
            "fixture_base_commit": base_commit,
            "task_definition_sha256": task_hash(task_id),
            "verifier_file_sha256": verifier_file_hash(task_id),
            "verifier_spec_sha256": canonical_hash(verifier_spec(task_id)),
            "api_exposure": execution_state["api_exposure"],
            "product_outcome_observed": True,
            "measurement_valid": measurement_valid,
            "measurement_invalid_reasons": measurement_invalid_reasons,
            "mixed_product_failure_and_measurement_gap": mixed_failure_gap,
            "verified_success": verified_success,
            "false_success": false_success,
            "task_success_before_measurement": task_success_before_measurement,
            "wall_time_contract_valid": wall_time_valid,
            "harness_cancel_sent_after_runtime_grace": harness_cancel_sent,
            "harness_cancel_raced_with_runtime_terminal": harness_cancel_raced,
            "budget_terminal_attribution_valid": budget_terminal_valid,
            "terminal": terminal,
            "completion": root_receipt,
            "completion_rejections": {
                "root": len(event_values(root_events, "completion_rejected")),
                "child": len(event_values(child_events, "completion_rejected")),
            },
            "task_contract_valid": task_contract_valid,
            "admission": admission,
            "scope_valid": scope_valid,
            "path_scope_valid": path_scope_valid,
            "t3_failure_then_recovery": t3_recovery,
            "exact_verifier": exact_verifier,
            "treatment_audit": treatment_result,
            "writer": writer,
            "git": git,
            "requests": accounting,
            "accounting_provenance": accounting_provenance,
            "child_accounting_provenance": child_accounting_provenance,
            "actor_usage": {
                "root": root_actor_usage,
                "child": child_actor_usage,
                "total_matches_terminal": actor_usage_matches,
                "response_count_matches_terminal": usage_response_count_matches,
                "response_counts_within_actor_requests": (
                    actor_response_counts_within_requests
                ),
                "actor_cost_available": False,
            },
            "tool_catalogs": catalogs,
            "tools": {
                "root_names": tool_names(root_events),
                "child_names": tool_names(child_events),
                "root_outcomes": tool_outcome_summary(root_events),
                "child_outcomes": tool_outcome_summary(child_events),
            },
            "model_failures": {
                "root": model_failure_summary(root_events, "root"),
                "child": model_failure_summary(child_events, "child"),
            },
            "event_counts": {
                "root": event_counts,
                "child": child_event_counts,
            },
            "run_store_facts": {
                "root_last_sequence": run.get("last_sequence"),
                "child_last_sequence": child_run.get("last_sequence")
                if child_run
                else None,
                "runtime_event_schema": RUNTIME_EVENT_SCHEMA,
                "run_api_schema": RUN_API_SCHEMA,
                "state_schema": state_schema,
            },
            "wall_time_ms": wall_time_ms,
            "secret_checks": {
                "key_in_argv": False,
                "key_in_protocol": False,
                "key_in_stderr": False,
                "key_in_fixture": False,
                "key_in_state": False,
                "proxy_or_custom_ca_inherited": False,
            },
            "stderr_sha256": sha256_bytes(stderr),
            "ephemeral_cleanup_pending": True,
        }
        result_redacted(record, key)
    record["ephemeral_cleanup_pending"] = False
    record["ephemeral_state_deleted"] = not ephemeral.exists()
    if not record["ephemeral_state_deleted"]:
        raise EvaluationError("ephemeral_state_not_deleted")
    record["wall_time_ms"] = int((time.monotonic() - arm_started) * 1000)
    if (
        record["wall_time_ms"]
        > RESOURCES["harness_wall_time_seconds"] * 1000
    ):
        record["wall_time_contract_valid"] = False
        record["task_success_before_measurement"] = False
        record["verified_success"] = False
        record["false_success"] = record["terminal"]["state"] == "completed"
    result_redacted(record, key)
    return record


def schedule(runs_per_cell: int = RUNS_PER_CELL) -> list[dict[str, Any]]:
    rounds = MANIFEST["experiment"]["task_round_order"]
    if runs_per_cell != len(rounds):
        raise EvaluationError("runs_per_cell_must_match_frozen_schedule")
    result = []
    position = 0
    for pair_index, task_order in enumerate(rounds, start=1):
        order = (
            ["single", "writer"]
            if pair_index % 2
            else ["writer", "single"]
        )
        for task_id in task_order:
            position += 1
            result.append(
                {
                    "schedule_position": position,
                    "task_id": task_id,
                    "pair_index": pair_index,
                    "order": order,
                }
            )
    return result


def metric_from_arm(arm: dict[str, Any], metric: str) -> float:
    if metric == "wall_time_ms":
        return float(arm["wall_time_ms"])
    if metric == "tokens":
        usage = arm["requests"]["usage"]
        return float(
            int(usage["input_tokens"]) + int(usage["output_tokens"])
        )
    if metric == "cost_nanousd":
        return float(arm["requests"]["cost_nanousd"])
    raise EvaluationError("unknown_metric")


def safe_ratio(candidate: float, baseline: float) -> float | None:
    if baseline == 0:
        return 0.0 if candidate == 0 else None
    return (candidate - baseline) / baseline


def median(values: list[float]) -> float | None:
    return statistics.median(values) if values else None


def distribution(values: list[float]) -> dict[str, float | int | None]:
    return {
        "count": len(values),
        "mean": statistics.fmean(values) if values else None,
        "median": median(values),
        "total": sum(values),
    }


def cell_summary(
    arms: list[dict[str, Any]], task_id: str, treatment: str
) -> dict[str, Any]:
    selected = [
        arm
        for arm in arms
        if arm["task_id"] == task_id and arm["treatment"] == treatment
    ]
    usage = {
        field: sum(
            int(arm["requests"]["usage"][field])
            for arm in selected
        )
        for field in USAGE_FIELDS
    }
    request_values = [
        float(
            int(arm["requests"]["root"]["started"] or 0)
            + int(arm["requests"]["child"]["started"] or 0)
        )
        for arm in selected
    ]
    token_values = [
        float(
            int(arm["requests"]["usage"]["input_tokens"])
            + int(arm["requests"]["usage"]["output_tokens"])
        )
        for arm in selected
    ]
    cost_values = [
        float(arm["requests"]["cost_nanousd"]) for arm in selected
    ]
    wall_values = [float(arm["wall_time_ms"]) for arm in selected]
    verified_success = sum(
        arm["verified_success"] is True for arm in selected
    )
    return {
        "task_id": task_id,
        "treatment": treatment,
        "runs": len(selected),
        "verified_success": verified_success,
        "verified_success_rate": (
            verified_success / len(selected) if selected else None
        ),
        "false_success": sum(
            arm["false_success"] is True for arm in selected
        ),
        "terminal_states": dict(
            sorted(Counter(arm["terminal"]["state"] for arm in selected).items())
        ),
        "measurement_valid": sum(
            arm["measurement_valid"] is True for arm in selected
        ),
        "physical_requests": sum(
            int(arm["requests"]["root"]["started"] or 0)
            + int(arm["requests"]["child"]["started"] or 0)
            for arm in selected
        ),
        "transport_retries": sum(
            int(arm["requests"]["transport_retries"] or 0)
            for arm in selected
        ),
        "runtime_retries": sum(
            int(arm["requests"]["runtime_retries"] or 0)
            for arm in selected
        ),
        "usage": usage,
        "primary_tokens": usage["input_tokens"] + usage["output_tokens"],
        "cost_nanousd": sum(
            int(arm["requests"]["cost_nanousd"]) for arm in selected
        ),
        "cost_nanocny": sum(
            int(arm["requests"]["cost_nanocny"]) for arm in selected
        ),
        "wall_time_ms": sum(int(arm["wall_time_ms"]) for arm in selected),
        "per_arm": {
            "physical_requests": distribution(request_values),
            "primary_tokens": distribution(token_values),
            "cost_nanousd": distribution(cost_values),
            "wall_time_ms": distribution(wall_values),
        },
        "root_tool_calls": sum(
            len(arm["tools"]["root_names"]) for arm in selected
        ),
        "child_tool_calls": sum(
            len(arm["tools"]["child_names"]) for arm in selected
        ),
    }


def pair_summary(pair: dict[str, Any]) -> dict[str, Any]:
    arms = {arm["treatment"]: arm for arm in pair["arms"]}
    deltas = {}
    directions = {}
    for metric in ("wall_time_ms", "tokens", "cost_nanousd"):
        single = metric_from_arm(arms["single"], metric)
        writer = metric_from_arm(arms["writer"], metric)
        delta = writer - single
        relative = safe_ratio(writer, single)
        deltas[metric] = {
            "absolute": delta,
            "relative": relative,
        }
        directions[metric] = (
            "writer_lower"
            if delta < 0
            else "writer_higher"
            if delta > 0
            else "equal"
        )
    return {
        "pair_id": pair["pair_id"],
        "task_id": pair["task_id"],
        "pair_index": pair["pair_index"],
        "attempt_index": pair["attempt_index"],
        "order": pair["order"],
        "verified_success": {
            treatment: arms[treatment]["verified_success"]
            for treatment in TREATMENTS
        },
        "false_success": {
            treatment: arms[treatment]["false_success"]
            for treatment in TREATMENTS
        },
        "deltas": deltas,
        "directions": directions,
    }


def paired_metric_summary(
    pairs: list[dict[str, Any]], task_ids: set[str], metric: str
) -> dict[str, Any]:
    in_scope = [pair for pair in pairs if pair["task_id"] in task_ids]
    eligible = [
        pair
        for pair in in_scope
        if all(
            arm.get("verified_success") is True
            for arm in pair.get("arms", [])
        )
        and {arm.get("treatment") for arm in pair.get("arms", [])}
        == set(TREATMENTS)
    ]
    values = [
        pair["comparison"]["deltas"][metric]["relative"]
        for pair in eligible
        if pair["comparison"]["deltas"][metric]["relative"] is not None
    ]
    directions = Counter(
        pair["comparison"]["directions"][metric]
        for pair in eligible
    )
    return {
        "pairs_in_scope": len(in_scope),
        "dual_verified_success_pairs": len(eligible),
        "pairs_with_defined_relative_delta": len(values),
        "excluded_non_dual_success_pairs": len(in_scope) - len(eligible),
        "paired_relative_mean": statistics.fmean(values) if values else None,
        "paired_relative_median": median(values),
        "directions": dict(sorted(directions.items())),
    }


def ratio_from_cells(
    cells: dict[tuple[str, str], dict[str, Any]],
    task_ids: set[str],
    metric: str,
) -> float | None:
    single = sum(cells[(task, "single")][metric] for task in task_ids)
    writer = sum(cells[(task, "writer")][metric] for task in task_ids)
    return safe_ratio(float(writer), float(single))


def writer_safety_reasons(arm: dict[str, Any]) -> list[str]:
    if arm.get("treatment") != "writer":
        return []
    reasons = []
    audit = arm.get("treatment_audit", {})
    writer = arm.get("writer", {})
    git = arm.get("git", {})
    counts = writer.get("event_counts", {})
    cleanup = writer.get("cleanup", {})
    seal_count = int(counts.get("agent_seal_committed") or 0)
    integration_count = int(
        counts.get("agent_integration_committed") or 0
    )
    if arm.get("false_success") is True:
        reasons.append("false_success")
    if audit.get("root_direct_write_violation") is True:
        reasons.append("root_direct_write")
    if arm.get("path_scope_valid") is False:
        reasons.append("path_scope_violation")
    if git.get("leak_free") is False:
        reasons.append("worktree_or_branch_leak")
    if int(writer.get("integration_failures") or 0) > 0:
        reasons.append("integration_failure")
    if seal_count > 0 and integration_count != 1:
        reasons.append("seal_without_unique_integration")
    if integration_count > 1:
        reasons.append("duplicate_integration")
    if integration_count:
        if writer.get("worktree_verifier_valid") is not True:
            reasons.append("unverified_integration")
        if git.get("root_clean") is not True:
            reasons.append("integrated_root_not_clean")
        if git.get("commit_count") != 1:
            reasons.append("integrated_commit_count")
        if git.get("head_commit") != writer.get("final_commit"):
            reasons.append("root_head_not_sealed_commit")
        if git.get("head_commit") != writer.get("root_head_commit"):
            reasons.append("root_head_not_integrated_commit")
    if arm.get("verified_success") is True:
        if integration_count != 1:
            reasons.append("successful_writer_missing_integration")
        if writer.get("successful_chain") is not True:
            reasons.append("successful_writer_incomplete_chain")
        if writer.get("worktree_verifier_valid") is not True:
            reasons.append("successful_writer_missing_worktree_verifier")
        if writer.get("latest_root_receipt") is not True:
            reasons.append("successful_writer_missing_latest_root_receipt")
    if cleanup.get("retained") is True:
        reasons.append("cleanup_retained")
    return list(dict.fromkeys(reasons))


def abort_is_hard_mechanism_failure(abort: dict[str, Any] | None) -> bool:
    return (
        isinstance(abort, dict)
        and abort.get("hard_mechanism_failure") is True
    )


def formal_decision(
    *,
    mechanism_gate_failed: bool,
    exact_pairs: bool,
    exact_cells: bool,
    hard_gate: bool,
    complex_benefit: bool,
    t1_control: bool,
    t1_success_non_regression: bool,
) -> str:
    if mechanism_gate_failed:
        return "reject_and_rework"
    if not exact_pairs or not exact_cells:
        return "hold_mechanism"
    if hard_gate and complex_benefit and t1_control:
        return "keep_default"
    if (
        hard_gate
        and complex_benefit
        and t1_success_non_regression
        and not t1_control
    ):
        return "shrink_on_demand"
    return "hold_mechanism"


def aggregate(
    accepted_pairs: list[dict[str, Any]],
    invalid_attempts: list[dict[str, Any]],
) -> dict[str, Any]:
    arms = [arm for pair in accepted_pairs for arm in pair["arms"]]
    cells_list = [
        cell_summary(arms, task_id, treatment)
        for task_id in TASK_IDS
        for treatment in TREATMENTS
    ]
    cells = {
        (cell["task_id"], cell["treatment"]): cell
        for cell in cells_list
    }
    pair_metrics = {
        scope: {
            metric: paired_metric_summary(
                accepted_pairs, task_set, metric
            )
            for metric in ("wall_time_ms", "tokens", "cost_nanousd")
        }
        for scope, task_set in {
            "all": set(TASK_IDS),
            "t1": {"t1"},
            "t2": {"t2"},
            "t3": {"t3"},
            "t2_t3": {"t2", "t3"},
        }.items()
    }
    per_task_non_regression = all(
        cells[(task, "writer")]["verified_success"]
        >= cells[(task, "single")]["verified_success"]
        for task in TASK_IDS
    ) if cells else False
    single_success = sum(
        cells[(task, "single")]["verified_success"] for task in TASK_IDS
    ) if cells else 0
    writer_success = sum(
        cells[(task, "writer")]["verified_success"] for task in TASK_IDS
    ) if cells else 0
    token_overhead = ratio_from_cells(
        cells, set(TASK_IDS), "primary_tokens"
    ) if cells else None
    cost_overhead = ratio_from_cells(
        cells, set(TASK_IDS), "cost_nanousd"
    ) if cells else None
    reliability_gate = MANIFEST["statistics"]["reliability_benefit"]
    reliability = (
        per_task_non_regression
        and writer_success - single_success
        >= reliability_gate["writer_success_delta_min"]
        and token_overhead is not None
        and token_overhead
        <= reliability_gate["aggregate_token_overhead_max"]
        and cost_overhead is not None
        and cost_overhead
        <= reliability_gate["aggregate_cost_overhead_max"]
    )
    complex_medians = {
        metric: pair_metrics["t2_t3"][metric]["paired_relative_median"]
        for metric in ("wall_time_ms", "tokens", "cost_nanousd")
    }
    efficiency_gate = MANIFEST["statistics"]["efficiency_benefit"]
    minimum_dual_success = efficiency_gate[
        "minimum_dual_success_pairs_per_complex_task"
    ]
    complex_dual_success_sample = all(
        pair_metrics[task]["wall_time_ms"]["dual_verified_success_pairs"]
        >= minimum_dual_success
        for task in ("t2", "t3")
    )
    complex_improvement = any(
        value is not None
        and value
        <= -efficiency_gate["t2_t3_any_paired_median_improvement_min"]
        for value in complex_medians.values()
    )
    complex_bounded = all(
        value is not None
        and value
        <= efficiency_gate["t2_t3_other_paired_median_regression_max"]
        for value in complex_medians.values()
    )
    each_complex_bounded = all(
        pair_metrics[task][metric]["paired_relative_median"] is not None
        and pair_metrics[task][metric]["paired_relative_median"]
        <= efficiency_gate["each_complex_task_metric_regression_max"]
        for task in ("t2", "t3")
        for metric in ("wall_time_ms", "tokens", "cost_nanousd")
    )
    efficiency = (
        per_task_non_regression
        and complex_dual_success_sample
        and complex_improvement
        and complex_bounded
        and each_complex_bounded
    )
    t1_success_non_regression = (
        cells[("t1", "writer")]["verified_success"]
        >= cells[("t1", "single")]["verified_success"]
    ) if cells else False
    t1_ratios = {
        "tokens": ratio_from_cells(cells, {"t1"}, "primary_tokens")
        if cells
        else None,
        "cost": ratio_from_cells(cells, {"t1"}, "cost_nanousd")
        if cells
        else None,
        "wall_time": ratio_from_cells(cells, {"t1"}, "wall_time_ms")
        if cells
        else None,
    }
    t1_gate = MANIFEST["statistics"]["t1_control"]
    t1_control = (
        t1_success_non_regression
        and pair_metrics["t1"]["wall_time_ms"][
            "dual_verified_success_pairs"
        ]
        >= t1_gate["minimum_dual_success_pairs"]
        and t1_ratios["tokens"] is not None
        and t1_ratios["tokens"]
        <= t1_gate["aggregate_token_overhead_max"]
        and t1_ratios["cost"] is not None
        and t1_ratios["cost"]
        <= t1_gate["aggregate_cost_overhead_max"]
        and t1_ratios["wall_time"] is not None
        and t1_ratios["wall_time"]
        <= t1_gate["aggregate_wall_time_overhead_max"]
    )
    complex_tasks = {"t2", "t3"}
    complex_success_non_regression = all(
        cells[(task, "writer")]["verified_success"]
        >= cells[(task, "single")]["verified_success"]
        for task in complex_tasks
    ) if cells else False
    complex_success_gain = sum(
        cells[(task, "writer")]["verified_success"]
        - cells[(task, "single")]["verified_success"]
        for task in complex_tasks
    ) if cells else 0
    complex_token_overhead = ratio_from_cells(
        cells, complex_tasks, "primary_tokens"
    ) if cells else None
    complex_cost_overhead = ratio_from_cells(
        cells, complex_tasks, "cost_nanousd"
    ) if cells else None
    complex_reliability = (
        complex_success_non_regression
        and complex_success_gain
        >= reliability_gate["writer_success_delta_min"]
        and complex_token_overhead is not None
        and complex_token_overhead
        <= reliability_gate["aggregate_token_overhead_max"]
        and complex_cost_overhead is not None
        and complex_cost_overhead
        <= reliability_gate["aggregate_cost_overhead_max"]
    )
    complex_benefit = complex_reliability or (
        complex_success_non_regression
        and complex_dual_success_sample
        and complex_improvement
        and complex_bounded
        and each_complex_bounded
    )

    invalid_arms = [
        arm for attempt in invalid_attempts for arm in attempt.get("arms", [])
    ]
    observed_arms = [*arms, *invalid_arms]
    trusted_product_arms = [
        arm
        for arm in observed_arms
        if arm.get("measurement_valid") is True
    ]
    false_success_count = sum(
        arm.get("false_success") is True for arm in trusted_product_arms
    )
    writer_false_success = sum(
        arm.get("false_success") is True
        for arm in trusted_product_arms
        if arm.get("treatment") == "writer"
    )
    harness_safety_failures = [
        arm
        for arm in observed_arms
        if arm.get("hard_safety_violation") is True
    ]
    safety_findings = [
        {
            "arm_id": arm.get("arm_id"),
            "task_id": arm.get("task_id"),
            "pair_index": arm.get("pair_index"),
            "attempt_index": arm.get("attempt_index"),
            "reasons": reasons,
        }
        for arm in trusted_product_arms
        if (reasons := writer_safety_reasons(arm))
    ]
    owner_gate = source_owner_audit() == expected_source_owners()
    exact_pairs = len(accepted_pairs) == len(TASK_IDS) * RUNS_PER_CELL
    exact_cells = all(
        cell["runs"] == RUNS_PER_CELL
        and cell["measurement_valid"] == RUNS_PER_CELL
        for cell in cells_list
    )
    hard_gate = (
        exact_pairs
        and exact_cells
        and false_success_count == 0
        and not harness_safety_failures
        and not safety_findings
        and owner_gate
        and per_task_non_regression
        and all(arm["measurement_valid"] for arm in arms)
        and all(
            not arm["requests"]["billing_unknown"] for arm in arms
        )
        and all(
            not arm["verified_success"]
            or arm["writer"]["latest_root_receipt"]
            for arm in arms
            if arm["treatment"] == "writer"
        )
    )
    mechanism_gate_failed = (
        false_success_count > 0
        or bool(harness_safety_failures)
        or bool(safety_findings)
        or not owner_gate
        or (exact_pairs and not per_task_non_regression)
    )
    decision = formal_decision(
        mechanism_gate_failed=mechanism_gate_failed,
        exact_pairs=exact_pairs,
        exact_cells=exact_cells,
        hard_gate=hard_gate,
        complex_benefit=complex_benefit,
        t1_control=t1_control,
        t1_success_non_regression=t1_success_non_regression,
    )
    product_eligible = exact_pairs and exact_cells and all(
        not attempt.get("mixed_product_failure_and_measurement_gap", False)
        for attempt in invalid_attempts
    )
    return {
        "product_metric_eligible": product_eligible,
        "hard_gate_met": hard_gate,
        "false_success": false_success_count,
        "writer_false_success": writer_false_success,
        "harness_safety_violations": len(harness_safety_failures),
        "writer_safety_violations": len(safety_findings),
        "writer_safety_findings": safety_findings,
        "source_owner_gate_met": owner_gate,
        "per_task_success_non_regression": per_task_non_regression,
        "reliability_benefit_met": reliability,
        "efficiency_benefit_met": efficiency,
        "complex_dual_success_sample_met": complex_dual_success_sample,
        "complex_task_benefit_met": complex_benefit,
        "t1_control_met": t1_control,
        "success": {
            "single": single_success,
            "writer": writer_success,
            "delta": writer_success - single_success,
        },
        "aggregate_overhead": {
            "tokens": token_overhead,
            "cost": cost_overhead,
        },
        "t1_overhead": t1_ratios,
        "complex_paired_medians": complex_medians,
        "cells": cells_list,
        "paired_metrics": pair_metrics,
        "decision": decision,
        "m6_b2_admitted": decision == "keep_default",
        "exact_pairs": exact_pairs,
        "exact_cells": exact_cells,
    }


def known_execution_metrics(
    accepted_pairs: list[dict[str, Any]],
    invalid_attempts: list[dict[str, Any]],
) -> dict[str, Any]:
    arms = [
        arm
        for pair in [*accepted_pairs, *invalid_attempts]
        for arm in pair.get("arms", [])
    ]
    unknown_billing_arms = sum(
        arm.get("requests", {}).get("billing_unknown") is True
        for arm in arms
    )
    known_cost_nanousd = sum(
        nonnegative_int_or_zero(
            arm.get("requests", {}).get("cost_nanousd")
        )
        for arm in arms
    )
    return {
        "arm_attempts": len(arms),
        "known_cost_nanousd": known_cost_nanousd,
        "known_cost_is_lower_bound": unknown_billing_arms > 0,
        "unknown_exposure_is_unbounded": unknown_billing_arms > 0,
        "reserved_unknown_exposure_nanousd": None
        if unknown_billing_arms
        else 0,
        "budget_exposure_nanousd": None
        if unknown_billing_arms
        else known_cost_nanousd,
        "known_cost_nanocny": sum(
            nonnegative_int_or_zero(
                arm.get("requests", {}).get("cost_nanocny")
            )
            for arm in arms
        ),
        "physical_requests_started": sum(
            nonnegative_int_or_zero(
                arm.get("requests", {}).get("root", {}).get("started")
            )
            + nonnegative_int_or_zero(
                arm.get("requests", {}).get("child", {}).get("started")
            )
            for arm in arms
        ),
        "unknown_billing_arms": unknown_billing_arms,
        "measurement_invalid_arms": sum(
            arm.get("measurement_valid") is not True for arm in arms
        ),
    }


def source_owner_audit() -> dict[str, Any]:
    queries = {
        "agent_runtime_structs": "pub struct AgentRuntime",
        "production_orchestrator_structs": (
            "pub struct ProductionAgentOrchestrator"
        ),
        "production_tool_name_constants": (
            "pub const PRODUCTION_TOOL_NAMES"
        ),
        "run_store_trait": "pub trait RunStore",
        "runtime_event_kind_enums": "pub enum RuntimeEventKind",
        "canonical_tool_catalog_functions": (
            "pub fn canonical_tool_catalog_sha256"
        ),
        "runtime_tool_definition_methods": "    pub fn tool_definitions(",
        "production_tool_executor_impls": (
            "impl ToolExecutor for ProductionToolExecutor"
        ),
    }
    result = {}
    rust_files = list((ROOT / "crates").rglob("*.rs"))
    for name, prefix in queries.items():
        count = 0
        for path in rust_files:
            for line in path.read_text(encoding="utf-8").splitlines():
                if line.startswith(prefix):
                    count += 1
        result[name] = count
    return result


def expected_source_owners() -> dict[str, int]:
    return {
        "agent_runtime_structs": 1,
        "production_orchestrator_structs": 1,
        "production_tool_name_constants": 1,
        "run_store_trait": 1,
        "runtime_event_kind_enums": 1,
        "canonical_tool_catalog_functions": 1,
        "runtime_tool_definition_methods": 1,
        "production_tool_executor_impls": 1,
    }


def validate_source_owners() -> None:
    actual = source_owner_audit()
    if actual != expected_source_owners():
        raise EvaluationError(
            "canonical_source_owner_count_invalid",
            {"expected": expected_source_owners(), "actual": actual},
        )


def freeze_identity(candidate: dict[str, Any] | None = None) -> dict[str, Any]:
    return {
        "schema": MANIFEST["schema"],
        "schema_versions": {
            "run_api": RUN_API_SCHEMA,
            "runtime_event": RUNTIME_EVENT_SCHEMA,
            "state": STATE_SCHEMA,
            "result": RESULT_SCHEMA,
        },
        "manifest_sha256": file_hash(MANIFEST_PATH),
        "manifest_content_sha256": manifest_content_hash(),
        "harness_sha256": file_hash(Path(__file__).resolve()),
        "canary_helper_sha256": file_hash(CANARY_PATH),
        "fixtures": {
            task_id: {
                "fixture_tree_sha256": fixture_hash(task_id),
                "verifier_file_sha256": verifier_file_hash(task_id),
                "verifier_spec_sha256": canonical_hash(
                    verifier_spec(task_id)
                ),
                "task_definition_sha256": task_hash(task_id),
            }
            for task_id in TASK_IDS
        },
        "schedule_sha256": canonical_hash(schedule()),
        "start_command_sha256": frozen_start_commands(),
        "source_owners": source_owner_audit(),
        "source_owner_gate_met": (
            source_owner_audit() == expected_source_owners()
        ),
        "app_server_argv": frozen_app_server_argv(),
        "app_server_argv_sha256": canonical_hash(frozen_app_server_argv()),
        "candidate": candidate,
    }


def manifest_freeze_fields() -> dict[str, Any]:
    validate_source_owners()
    frozen_catalogs = copy.deepcopy(
        MANIFEST.get("frozen_hashes", {}).get(
            "ordered_tool_definition_sha256"
        )
    )
    expected_catalog_keys = {
        "single_root",
        "writer_root",
        "writer_child",
        "terminal_empty",
    }
    if (
        not isinstance(frozen_catalogs, dict)
        or set(frozen_catalogs) != set(TASK_IDS)
        or any(
            not isinstance(frozen_catalogs.get(task_id), dict)
            or set(frozen_catalogs[task_id]) != expected_catalog_keys
            for task_id in TASK_IDS
        )
    ):
        raise EvaluationError("frozen_catalog_definitions_missing")
    return {
        "manifest_content_sha256_excluding_frozen_hashes": (
            manifest_content_hash()
        ),
        "schema_versions": {
            "run_api": RUN_API_SCHEMA,
            "runtime_event": RUNTIME_EVENT_SCHEMA,
            "state": STATE_SCHEMA,
            "result": RESULT_SCHEMA,
        },
        "harness_sha256": file_hash(Path(__file__).resolve()),
        "canary_helper_sha256": file_hash(CANARY_PATH),
        "schedule_sha256": canonical_hash(schedule()),
        "app_server_argv_sha256": canonical_hash(frozen_app_server_argv()),
        "source_owners": expected_source_owners(),
        "start_command_sha256": frozen_start_commands(),
        "fixture_tree_sha256": {
            task_id: fixture_hash(task_id) for task_id in TASK_IDS
        },
        "task_definition_sha256": {
            task_id: task_hash(task_id) for task_id in TASK_IDS
        },
        "verifier_file_sha256": {
            task_id: verifier_file_hash(task_id) for task_id in TASK_IDS
        },
        "verifier_spec_sha256": {
            task_id: canonical_hash(verifier_spec(task_id))
            for task_id in TASK_IDS
        },
        "ordered_tool_definition_sha256": frozen_catalogs,
    }


def validate_frozen_hashes() -> None:
    validate_source_owners()
    frozen = MANIFEST.get("frozen_hashes", {})
    actual_fixtures = {
        task_id: fixture_hash(task_id) for task_id in TASK_IDS
    }
    actual_tasks = {
        task_id: task_hash(task_id) for task_id in TASK_IDS
    }
    actual_verifier_files = {
        task_id: verifier_file_hash(task_id) for task_id in TASK_IDS
    }
    actual_verifier_specs = {
        task_id: canonical_hash(verifier_spec(task_id))
        for task_id in TASK_IDS
    }
    expected_catalog_keys = {
        "single_root",
        "writer_root",
        "writer_child",
        "terminal_empty",
    }
    catalog_maps_valid = True
    for field in ("ordered_tool_definition_sha256",):
        task_catalogs = frozen.get(field, {})
        catalog_maps_valid = catalog_maps_valid and (
            isinstance(task_catalogs, dict)
            and set(task_catalogs) == set(TASK_IDS)
        )
        if not isinstance(task_catalogs, dict):
            continue
        for task_id in TASK_IDS:
            values = task_catalogs.get(task_id, {})
            catalog_maps_valid = catalog_maps_valid and (
                isinstance(values, dict)
                and set(values) == expected_catalog_keys
                and all(
                    isinstance(value, str)
                    and value.startswith("sha256:")
                    and raw_sha256(value.removeprefix("sha256:"))
                    for value in values.values()
                )
                and values.get("terminal_empty") == canonical_hash([])
            )
    placeholder_free = "TO_BE_FROZEN" not in json.dumps(
        frozen, ensure_ascii=False, sort_keys=True
    )
    checks = {
        "manifest_content": frozen.get(
            "manifest_content_sha256_excluding_frozen_hashes"
        )
        == manifest_content_hash(),
        "fixtures": frozen.get("fixture_tree_sha256")
        == actual_fixtures,
        "tasks": frozen.get("task_definition_sha256") == actual_tasks,
        "verifier_files": frozen.get("verifier_file_sha256")
        == actual_verifier_files,
        "verifier_specs": frozen.get("verifier_spec_sha256")
        == actual_verifier_specs,
        "schema_versions": frozen.get("schema_versions")
        == {
            "run_api": RUN_API_SCHEMA,
            "runtime_event": RUNTIME_EVENT_SCHEMA,
            "state": STATE_SCHEMA,
            "result": RESULT_SCHEMA,
        },
        "harness": frozen.get("harness_sha256")
        == file_hash(Path(__file__).resolve()),
        "canary": frozen.get("canary_helper_sha256")
        == file_hash(CANARY_PATH),
        "schedule": frozen.get("schedule_sha256")
        == canonical_hash(schedule()),
        "start_commands": frozen.get("start_command_sha256")
        == frozen_start_commands(),
        "app_server_argv": frozen.get("app_server_argv_sha256")
        == canonical_hash(frozen_app_server_argv()),
        "source_owners": frozen.get("source_owners")
        == expected_source_owners(),
        "catalog_maps": catalog_maps_valid,
        "placeholder_free": placeholder_free,
        "experiment_shape": (
            MANIFEST.get("experiment", {}).get("model") == MODEL
            and MANIFEST.get("experiment", {}).get("runs_per_task_treatment")
            == RUNS_PER_CELL
            and MANIFEST.get("experiment", {}).get("accepted_pairs")
            == len(TASK_IDS) * RUNS_PER_CELL
            and MANIFEST.get("experiment", {}).get("accepted_arms")
            == len(TASK_IDS) * len(TREATMENTS) * RUNS_PER_CELL
            and MANIFEST.get("experiment", {}).get("pair_attempts_max")
            == MAX_PAIR_ATTEMPTS
            and RESOURCES.get("cargo_incremental") == "0"
            and str(RESOURCES.get("cargo_target_dir", "")).startswith(
                "/private/tmp/"
            )
        ),
    }
    failed = [name for name, valid in checks.items() if not valid]
    if failed:
        raise EvaluationError(
            "frozen_hash_mismatch", {"axes": failed}
        )


def atomic_write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    handle, temporary_raw = tempfile.mkstemp(
        prefix=path.name + ".", dir=path.parent
    )
    temporary = Path(temporary_raw)
    try:
        with os.fdopen(handle, "wb") as stream:
            stream.write(canonical_bytes(payload))
            stream.write(b"\n")
            stream.flush()
            os.fsync(stream.fileno())
        os.chmod(temporary, 0o600)
        os.replace(temporary, path)
        directory = os.open(path.parent, os.O_RDONLY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        temporary.unlink(missing_ok=True)


def partial_payload(
    identity: dict[str, Any],
    accepted_pairs: list[dict[str, Any]],
    invalid_attempts: list[dict[str, Any]],
    mode: str = "partial",
    abort: dict[str, Any] | None = None,
) -> dict[str, Any]:
    payload = {
        "schema": RESULT_SCHEMA,
        "mode": mode,
        "identity": identity,
        "accepted_pairs": accepted_pairs,
        "invalid_attempts": invalid_attempts,
        "execution": known_execution_metrics(
            accepted_pairs, invalid_attempts
        ),
    }
    if abort:
        payload["abort"] = abort
    return payload


def finalize_result(
    args: argparse.Namespace,
    key: str,
    identity: dict[str, Any],
    accepted_pairs: list[dict[str, Any]],
    invalid_attempts: list[dict[str, Any]],
    suite_started: float,
    mode: str,
    abort: dict[str, Any] | None = None,
) -> dict[str, Any]:
    aggregate_result = aggregate(accepted_pairs, invalid_attempts)
    if abort is not None:
        hard_mechanism_abort = abort_is_hard_mechanism_failure(abort)
        aggregate_result["product_metric_eligible"] = False
        aggregate_result["hard_gate_met"] = False
        aggregate_result["m6_b2_admitted"] = False
        aggregate_result["hard_mechanism_abort"] = hard_mechanism_abort
        aggregate_result["decision"] = (
            "reject_and_rework"
            if hard_mechanism_abort
            or aggregate_result["false_success"]
            or aggregate_result["harness_safety_violations"]
            or aggregate_result["writer_safety_violations"]
            or not aggregate_result["source_owner_gate_met"]
            else "hold_mechanism"
        )
    result = {
        "schema": RESULT_SCHEMA,
        "mode": mode,
        "created_at_utc": time.strftime(
            "%Y-%m-%dT%H:%M:%SZ", time.gmtime()
        ),
        "key_accessed": True,
        "identity": identity,
        "model": MODEL,
        "api_surface": "standard_chat",
        "runs_per_cell": RUNS_PER_CELL,
        "accepted_pairs": accepted_pairs,
        "invalid_attempts": invalid_attempts,
        "execution": known_execution_metrics(
            accepted_pairs, invalid_attempts
        ),
        "aggregate": aggregate_result,
        "suite_duration_seconds": round(
            time.monotonic() - suite_started, 3
        ),
        "interpretation": {
            "claim": "same-revision Writer treatment effect only",
            "statistical_claim": (
                "repeated paired direction and median, not significance; "
                "efficiency uses dual-verified-success pairs only"
            ),
            "root_write_boundary": (
                "Writer coordinator write tools are removed by Host actor "
                "authority; any observed root write is a hard mechanism failure"
            ),
            "transport_retry_boundary": (
                "app-server was configured with --transport-max-retries=1; "
                "accepted arms also require observed total retries <=1 inside "
                "the shared physical budget"
            ),
            "cost_boundary": (
                "suite gate applies to exact known cost before each arm; any "
                "unknown billing is retained as a lower bound and stops the "
                "suite immediately because its exposure has no proven bound"
            ),
        },
    }
    if abort is not None:
        result["abort"] = abort
    result_redacted(result, key)
    atomic_write_json(args.output, result)
    if stat.S_IMODE(args.output.stat().st_mode) != 0o600:
        raise EvaluationError("result_mode_not_0600")
    if key.encode() in args.output.read_bytes():
        raise EvaluationError("key_in_result_file")
    args.output.with_suffix(args.output.suffix + ".partial").unlink(
        missing_ok=True
    )
    return result


def dry_plan(binary: Path | None, revision: str | None) -> dict[str, Any]:
    validate_frozen_hashes()
    candidate = (
        binary_identity(binary, revision)
        if binary is not None and revision is not None
        else None
    )
    return {
        "schema": RESULT_SCHEMA,
        "mode": "dry_run",
        "paid_request_started": False,
        "key_accessed": False,
        "accepted_pairs": len(TASK_IDS) * RUNS_PER_CELL,
        "accepted_arms": len(TASK_IDS) * len(TREATMENTS) * RUNS_PER_CELL,
        "maximum_pair_attempts": len(TASK_IDS)
        * RUNS_PER_CELL
        * MAX_PAIR_ATTEMPTS,
        "maximum_arm_attempts": len(TASK_IDS)
        * len(TREATMENTS)
        * RUNS_PER_CELL
        * MAX_PAIR_ATTEMPTS,
        "schedule": schedule(),
        "resources": RESOURCES,
        "freeze_identity": freeze_identity(candidate),
        "same_binary_treatments": True,
        "transport_policy_note": (
            "the frozen app-server process is launched with "
            "--transport-max-retries=1; the shared 10-request physical budget "
            "includes every initial request and retry"
        ),
    }


class HarnessSelfTests(unittest.TestCase):
    @staticmethod
    def complete_accounting(*, sealed: bool = True) -> dict[str, Any]:
        usage = {
            "input_tokens": 10,
            "output_tokens": 2,
            "cache_hit_tokens": 0,
            "cache_miss_tokens": 10,
            "cache_write_tokens": 0,
            "reasoning_tokens": 0,
            "reasoning_replay_tokens": 0,
        }
        return {
            "hard_request_limit": 10,
            "root": {
                "started": 1,
                "completed": 1,
                "in_flight": 0,
                "retries": 0,
            },
            "child": {
                "started": 0,
                "completed": 0,
                "in_flight": 0,
                "retries": 0,
            },
            "transport_retries": 0,
            "runtime_retries": 0,
            "sealed_denied": 0,
            "exhausted_denied": 0,
            "budget_exhausted": False,
            "sealed": sealed,
            "complete": True,
            "usage_complete": True,
            "usage_missing": False,
            "usage_incomplete": False,
            "billing_unknown": False,
            "unpriced": False,
            "usage_responses": 1,
            "usage_missing_responses": 0,
            "incomplete_responses": 0,
            "billing_unknown_attempts": 0,
            "unpriced_usage_responses": 0,
            "records_after_seal": 0,
            "usage": usage,
            "surface_usage": [
                {
                    "surface": "standard_chat",
                    "model": MODEL,
                    "response_count": 1,
                    "usage_response_count": 1,
                    "usage": usage,
                    "cost_nanousd": 100,
                    "cost_nanocny": 700,
                }
            ],
            "cost_nanousd": 100,
            "cost_nanocny": 700,
        }

    def test_terminal_accounting_has_canonical_provenance(self) -> None:
        accounting = self.complete_accounting()
        terminal = {"state": "blocked", "reason": "fixture"}
        run = {
            "run_id": "run-1",
            "last_sequence": 2,
            "terminal": terminal,
            "accounting": copy.deepcopy(accounting),
        }
        events = [
            {"sequence": 1, "event": {"kind": "run_created"}},
            {
                "sequence": 2,
                "event": {
                    "kind": "terminal",
                    "outcome": {
                        "run_id": "run-1",
                        "terminal": terminal,
                        "accounting": accounting,
                    },
                },
            },
        ]
        summary, provenance, reasons = canonical_terminal_accounting(
            run, events
        )
        self.assertEqual(reasons, [])
        self.assertEqual(provenance["source"], "canonical_terminal_event")
        self.assertTrue(provenance["terminal_is_last"])
        self.assertTrue(provenance["terminal_matches_run_view"])
        self.assertTrue(
            provenance["terminal_accounting_matches_run_view"]
        )
        self.assertTrue(accounting_valid(summary)[0])

        mismatched_run = copy.deepcopy(run)
        mismatched_run["accounting"]["cost_nanousd"] = 101
        selected, mismatch, reasons = canonical_terminal_accounting(
            mismatched_run, events
        )
        self.assertEqual(selected["cost_nanousd"], 101)
        self.assertTrue(selected["billing_unknown"])
        self.assertTrue(selected["known_cost_is_lower_bound"])
        self.assertEqual(mismatch["source"], "terminal_event_unverified")
        self.assertFalse(
            mismatch["terminal_accounting_matches_run_view"]
        )
        self.assertIn("terminal_accounting_run_view_mismatch", reasons)

        non_terminal_tail = [
            *events,
            {"sequence": 3, "event": {"kind": "diagnostic"}},
        ]
        tail_run = copy.deepcopy(run)
        tail_run["last_sequence"] = 3
        selected, non_terminal, reasons = canonical_terminal_accounting(
            tail_run, non_terminal_tail
        )
        self.assertEqual(non_terminal["source"], "terminal_event_unverified")
        self.assertFalse(non_terminal["terminal_is_last"])
        self.assertTrue(selected["billing_unknown"])
        self.assertIn("terminal_accounting_provenance", reasons)

        expensive_duplicate = copy.deepcopy(events[-1])
        expensive_accounting = expensive_duplicate["event"]["outcome"][
            "accounting"
        ]
        expensive_accounting["cost_nanousd"] = 900
        expensive_accounting["cost_nanocny"] = 6_300
        expensive_accounting["surface_usage"][0]["cost_nanousd"] = 900
        expensive_accounting["surface_usage"][0]["cost_nanocny"] = 6_300
        duplicated = [*events, expensive_duplicate]
        selected, duplicate_provenance, reasons = canonical_terminal_accounting(
            run, duplicated
        )
        self.assertEqual(duplicate_provenance["terminal_count"], 2)
        self.assertEqual(selected["cost_nanousd"], 900)
        self.assertTrue(selected["billing_unknown"])
        self.assertIn("terminal_accounting_provenance", reasons)

    def test_unsealed_and_partial_accounting_are_lower_bounds(self) -> None:
        run = {
            "accounting": self.complete_accounting(sealed=False),
            "last_sequence": 1,
        }
        summary = accounting_summary(run)
        valid, reasons = accounting_valid(summary)
        self.assertFalse(valid)
        self.assertIn("sealed", reasons)
        self.assertIn("billing_known", reasons)
        self.assertTrue(summary["billing_unknown"])
        self.assertTrue(summary["known_cost_is_lower_bound"])

        partial = provisional_accounting_summary(
            {
                "accounting": self.complete_accounting(),
                "last_sequence": 2,
            }
        )
        self.assertFalse(partial["sealed"])
        self.assertFalse(partial["complete"])
        self.assertTrue(partial["billing_unknown"])
        self.assertTrue(partial["known_cost_is_lower_bound"])
        self.assertTrue(partial["request_count_unknown"])
        self.assertEqual(partial["cost_nanousd"], 100)

        after_seal = self.complete_accounting()
        after_seal["records_after_seal"] = 1
        summary = accounting_summary({"accounting": after_seal})
        self.assertTrue(summary["billing_unknown"])
        self.assertTrue(summary["known_cost_is_lower_bound"])

    def test_actor_usage_counts_responses_and_rejects_bad_shape(self) -> None:
        usage = self.complete_accounting()["usage"]
        valid = actor_usage(
            [
                {
                    "event": {
                        "kind": "model_response_committed",
                        "output": {"usage": usage},
                    }
                }
            ]
        )
        self.assertEqual(valid["usage"], usage)
        self.assertEqual(valid["response_count"], 1)
        self.assertTrue(valid["shape_valid"])

        malformed = copy.deepcopy(usage)
        malformed.pop("reasoning_replay_tokens")
        invalid = actor_usage(
            [
                {
                    "event": {
                        "kind": "model_response_committed",
                        "output": {"usage": malformed},
                    }
                }
            ]
        )
        self.assertEqual(invalid["response_count"], 1)
        self.assertFalse(invalid["shape_valid"])

        negative = copy.deepcopy(usage)
        negative["output_tokens"] = -1
        invalid = actor_usage(
            [
                {
                    "event": {
                        "kind": "model_response_committed",
                        "output": {"usage": negative},
                    }
                }
            ]
        )
        self.assertFalse(invalid["shape_valid"])

    def test_fixture_git_identities_are_reproducible(self) -> None:
        for task_id in TASK_IDS:
            with self.subTest(task=task_id), tempfile.TemporaryDirectory() as raw:
                _, base_commit, initial_tree = initialize_workspace(
                    task_id,
                    Path(raw),
                )
                self.assertEqual(
                    base_commit,
                    MANIFEST["tasks"][task_id]["fixture_base_commit"],
                )
                self.assertEqual(initial_tree, fixture_hash(task_id))

    def test_fixtures_start_failing_and_known_fixes_pass(self) -> None:
        fixes = {
            "t1": {
                "format_bytes.py": (
                    fixture_path("t1")
                    .joinpath("format_bytes.py")
                    .read_text(encoding="utf-8")
                    .replace("value /= 1000", "value /= 1024")
                )
            },
            "t2": {
                "agent_profile.py": (
                    "from __future__ import annotations\n"
                    "from typing import Any\n\n"
                    "def merge_profile(base: dict[str, Any], override: dict[str, Any]) -> dict[str, Any]:\n"
                    "    result = dict(base)\n"
                    "    for key, value in override.items():\n"
                    "        if isinstance(value, dict) and isinstance(result.get(key), dict):\n"
                    "            result[key] = merge_profile(result[key], value)\n"
                    "        else:\n"
                    "            result[key] = value\n"
                    "    return result\n"
                ),
                "test_agent_profile.py": (
                    fixture_path("t2")
                    .joinpath("test_agent_profile.py")
                    .read_text(encoding="utf-8")
                    + "\n\ndef test_nested_limits_are_merged():\n"
                    "    assert merge_profile({'limits': {'a': 1, 'b': 2}}, {'limits': {'a': 3}}) == {'limits': {'a': 3, 'b': 2}}\n"
                    "\n\ndef test_inputs_are_not_mutated():\n"
                    "    base = {'limits': {'a': 1}}\n"
                    "    override = {'limits': {'b': 2}}\n"
                    "    merge_profile(base, override)\n"
                    "    assert base == {'limits': {'a': 1}}\n"
                    "    assert override == {'limits': {'b': 2}}\n"
                ),
            },
            "t3": {
                "retry_window.py": (
                    fixture_path("t3")
                    .joinpath("retry_window.py")
                    .read_text(encoding="utf-8")
                    .replace("2 ** (attempt + 1)", "2 ** attempt")
                )
            },
        }
        for task_id in TASK_IDS:
            with self.subTest(task=task_id), tempfile.TemporaryDirectory() as raw:
                workspace = Path(raw) / "workspace"
                shutil.copytree(fixture_path(task_id), workspace)
                self.assertFalse(
                    run_external_verifier(task_id, workspace)["passed"]
                )
                for relative, body in fixes[task_id].items():
                    (workspace / relative).write_text(body, encoding="utf-8")
                self.assertTrue(
                    run_external_verifier(task_id, workspace)["passed"]
                )

    def test_schedule_is_balanced_and_rotated(self) -> None:
        planned = schedule()
        self.assertEqual(len(planned), 18)
        arm_plan = [
            (pair["schedule_position"], 1, arm_position, treatment)
            for pair in planned
            for arm_position, treatment in enumerate(pair["order"], start=1)
        ]
        self.assertEqual(len(arm_plan), 36)
        self.assertEqual({attempt for _, attempt, _, _ in arm_plan}, {1})
        for task_id in TASK_IDS:
            task_pairs = [
                item for item in planned if item["task_id"] == task_id
            ]
            self.assertEqual(len(task_pairs), 6)
            self.assertEqual(
                Counter(tuple(item["order"]) for item in task_pairs),
                Counter(
                    {
                        ("single", "writer"): 3,
                        ("writer", "single"): 3,
                    }
                ),
            )

    def test_task_is_identical_across_treatments(self) -> None:
        for task_id in TASK_IDS:
            single = start_command(
                task_id, "single", Path("/tmp/workspace"), "single"
            )
            writer = start_command(
                task_id, "writer", Path("/tmp/workspace"), "writer"
            )
            self.assertEqual(
                single["command"]["task"], writer["command"]["task"]
            )
            self.assertEqual(
                single["command"]["tool_policy"],
                writer["command"]["tool_policy"],
            )
            self.assertEqual(
                single["command"]["max_api_requests"],
                writer["command"]["max_api_requests"],
            )
            single_limits = dict(single["command"]["limits"])
            writer_limits = dict(writer["command"]["limits"])
            for field in ("max_depth", "max_concurrent_children"):
                single_limits.pop(field)
                writer_limits.pop(field)
            self.assertEqual(single_limits, writer_limits)

    def test_manifest_resource_bounds_are_frozen(self) -> None:
        validate_frozen_hashes()
        self.assertEqual(
            RESOURCES["max_physical_api_requests_per_arm"], 10
        )
        self.assertLessEqual(
            RESOURCES["harness_wall_time_seconds"], 240
        )
        self.assertEqual(RESOURCES["suite_known_cost_usd"], 0.5)
        self.assertEqual(MAX_PAIR_ATTEMPTS, 1)
        self.assertEqual(MODEL, "deepseek-v4-flash")
        self.assertEqual(
            RESOURCES["production_verifier_gate_timeout_ms"], 600_000
        )
        self.assertLess(
            RESOURCES["runtime_wall_time_seconds"]
            + RESOURCES["runtime_cancel_grace_seconds"],
            RESOURCES["harness_wall_time_seconds"],
        )
        self.assertLessEqual(
            RESOURCES["stdio_poll_timeout_seconds"],
            RESOURCES["harness_wall_time_seconds"]
            - RESOURCES["runtime_wall_time_seconds"]
            - RESOURCES["runtime_cancel_grace_seconds"],
        )
        self.assertEqual(WRITER_CHILD_TOOLS, sorted(WRITER_CHILD_TOOLS))

    def test_atomic_output_is_0600(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            path = Path(raw) / "result.json"
            atomic_write_json(path, {"ok": True})
            self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o600)
            self.assertEqual(json.loads(path.read_text()), {"ok": True})

    def test_failure_accounting_is_conservative(self) -> None:
        possible = failure_arm_record(
            "t1",
            "single",
            1,
            1,
            ["single", "writer"],
            1,
            "stdio_timeout",
            "possible",
            10,
        )
        self.assertTrue(possible["requests"]["billing_unknown"])
        self.assertTrue(possible["requests"]["known_cost_is_lower_bound"])
        self.assertFalse(possible["product_outcome_observed"])
        before_api = failure_arm_record(
            "t1",
            "single",
            1,
            1,
            ["single", "writer"],
            1,
            "app_server_launch_failed",
            "none",
            10,
        )
        self.assertFalse(before_api["requests"]["billing_unknown"])
        secret_failure = failure_arm_record(
            "t1",
            "writer",
            1,
            1,
            ["single", "writer"],
            2,
            "key_in_stderr",
            "possible",
            10,
        )
        self.assertTrue(secret_failure["hard_safety_violation"])

    def test_non_exact_billing_stops_with_unbounded_exposure(self) -> None:
        summary = accounting_summary(
            {
                "accounting": {
                    "billing_unknown": False,
                    "unpriced": True,
                    "cost_nanousd": 0,
                    "cost_nanocny": 0,
                    "root": {},
                    "child": {},
                }
            }
        )
        self.assertTrue(summary["billing_unknown"])
        self.assertTrue(summary["known_cost_is_lower_bound"])
        self.assertTrue(summary["request_count_unknown"])
        metrics = known_execution_metrics(
            [],
            [{"arms": [{"requests": summary}]}],
        )
        self.assertEqual(metrics["unknown_billing_arms"], 1)
        self.assertTrue(metrics["unknown_exposure_is_unbounded"])
        self.assertIsNone(metrics["reserved_unknown_exposure_nanousd"])
        self.assertIsNone(metrics["budget_exposure_nanousd"])

    def test_accounting_rejects_internally_inconsistent_counts(self) -> None:
        too_many_responses = self.complete_accounting()
        too_many_responses["usage_responses"] = 2
        too_many_responses["surface_usage"][0]["response_count"] = 2
        too_many_responses["surface_usage"][0]["usage_response_count"] = 2
        valid, reasons = accounting_valid(
            summarize_accounting(too_many_responses)
        )
        self.assertFalse(valid)
        self.assertIn("response_count_within_requests", reasons)

        negative_cost = self.complete_accounting()
        negative_cost["cost_nanousd"] = -1
        negative_cost["surface_usage"][0]["cost_nanousd"] = -1
        valid, reasons = accounting_valid(summarize_accounting(negative_cost))
        self.assertFalse(valid)
        self.assertIn("numeric_shape", reasons)

        hidden_gap = self.complete_accounting()
        hidden_gap["billing_unknown_attempts"] = 1
        valid, reasons = accounting_valid(summarize_accounting(hidden_gap))
        self.assertFalse(valid)
        self.assertIn("usage_gap_counters_zero", reasons)

        corrupt_surface = self.complete_accounting()
        corrupt_surface["surface_usage"].append("corrupt")
        valid, reasons = accounting_valid(
            summarize_accounting(corrupt_surface)
        )
        self.assertFalse(valid)
        self.assertIn("numeric_shape", reasons)

    def test_mixed_product_failure_and_gap_is_measurement_invalid(self) -> None:
        observed_failure = {
            "measurement_valid": True,
            "product_outcome_observed": True,
            "task_success_before_measurement": False,
            "api_exposure": "accounted",
        }
        pre_api_gap = {
            "measurement_valid": False,
            "product_outcome_observed": False,
            "task_success_before_measurement": None,
            "api_exposure": "none",
        }
        classified = pair_measurement_classification(
            [observed_failure, pre_api_gap]
        )
        self.assertTrue(
            classified["mixed_product_failure_and_measurement_gap"]
        )
        self.assertFalse(classified["measurement_valid"])

    def test_efficiency_excludes_non_dual_success_pairs(self) -> None:
        def paired(
            pair_id: str, single_success: bool, writer_success: bool, delta: float
        ) -> dict[str, Any]:
            arms = [
                {"treatment": "single", "verified_success": single_success},
                {"treatment": "writer", "verified_success": writer_success},
            ]
            return {
                "pair_id": pair_id,
                "task_id": "t2",
                "arms": arms,
                "comparison": {
                    "deltas": {
                        "wall_time_ms": {"relative": delta},
                    },
                    "directions": {
                        "wall_time_ms": (
                            "writer_lower" if delta < 0 else "writer_higher"
                        )
                    },
                },
            }

        summary = paired_metric_summary(
            [
                paired("success", True, True, -0.2),
                paired("fast-failure", True, False, -0.9),
            ],
            {"t2"},
            "wall_time_ms",
        )
        self.assertEqual(summary["dual_verified_success_pairs"], 1)
        self.assertEqual(summary["excluded_non_dual_success_pairs"], 1)
        self.assertEqual(summary["paired_relative_median"], -0.2)

    def test_default_requires_complex_task_benefit(self) -> None:
        common = {
            "mechanism_gate_failed": False,
            "exact_pairs": True,
            "exact_cells": True,
            "hard_gate": True,
            "t1_control": True,
            "t1_success_non_regression": True,
        }
        self.assertEqual(
            formal_decision(**common, complex_benefit=False),
            "hold_mechanism",
        )
        self.assertEqual(
            formal_decision(**common, complex_benefit=True),
            "keep_default",
        )

    def test_writer_safety_matrix_catches_scope_and_integration(self) -> None:
        reasons = writer_safety_reasons(
            {
                "treatment": "writer",
                "product_outcome_observed": True,
                "false_success": False,
                "path_scope_valid": False,
                "treatment_audit": {
                    "root_direct_write_violation": True,
                },
                "git": {"leak_free": True},
                "writer": {
                    "integration_failures": 1,
                    "event_counts": {
                        "agent_integration_committed": 0,
                    },
                },
                "verified_success": False,
            }
        )
        self.assertIn("path_scope_violation", reasons)
        self.assertIn("integration_failure", reasons)
        self.assertIn("root_direct_write", reasons)

        incomplete = {
            "treatment": "writer",
            "product_outcome_observed": False,
            "false_success": False,
            "path_scope_valid": True,
            "treatment_audit": {"root_direct_write_violation": False},
            "git": {"leak_free": True},
            "writer": {
                "integration_failures": 0,
                "event_counts": {
                    "agent_seal_committed": 1,
                    "agent_integration_committed": 0,
                },
                "cleanup": {"retained": True},
            },
            "verified_success": False,
        }
        incomplete_reasons = writer_safety_reasons(incomplete)
        self.assertIn(
            "seal_without_unique_integration", incomplete_reasons
        )
        self.assertIn("cleanup_retained", incomplete_reasons)

    def test_abort_decision_distinguishes_mechanism_from_measurement(self) -> None:
        self.assertTrue(
            abort_is_hard_mechanism_failure(
                {
                    "reasons": ["writer_cleanup_retained"],
                    "hard_mechanism_failure": True,
                }
            )
        )
        self.assertFalse(
            abort_is_hard_mechanism_failure(
                {
                    "reasons": ["writer_child_mode_identity"],
                    "hard_mechanism_failure": False,
                }
            )
        )
        self.assertFalse(
            abort_is_hard_mechanism_failure(
                {
                    "reasons": ["writer_cleanup_scope_hash"],
                    "hard_mechanism_failure": False,
                }
            )
        )

    def test_manifest_freeze_output_matches_frozen_field_shape(self) -> None:
        self.assertEqual(
            manifest_freeze_fields(),
            MANIFEST["frozen_hashes"],
        )

    def test_canonical_source_owners_are_singular(self) -> None:
        self.assertEqual(source_owner_audit(), expected_source_owners())

    def test_runtime_terminal_catalog_is_empty_only_at_the_end(self) -> None:
        definition = {
            "name": "read_file",
            "description": "读取 UTF-8 文件",
            "input_schema": {
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"],
                "additionalProperties": False,
            },
        }
        expected_runtime = sha256_bytes(
            json.dumps(
                [definition], ensure_ascii=False, separators=(",", ":")
            ).encode("utf-8")
        )
        empty = canonical_hash([])
        self.assertNotEqual(canonical_hash([definition]), expected_runtime)

        def catalog(
            request_number: int,
            attempt: int,
            tools: list[dict[str, Any]],
            source: str,
        ) -> dict[str, Any]:
            return request_catalog_summary(
                {
                    "actor": {"kind": "root"},
                    "request_number": request_number,
                    "attempt": attempt,
                    "tools": tools,
                },
                "root",
                source,
                request_number + attempt,
                f"attempt-{request_number}-{attempt}",
                (
                    f"attempt-{request_number}-{attempt - 1}"
                    if attempt > 0
                    else None
                ),
            )

        valid = catalog_identity_audit(
            [
                catalog(1, 0, [definition], "model_request_prepared"),
                catalog(
                    1,
                    1,
                    [definition],
                    "model_request_failed.retry.prepared",
                ),
                catalog(2, 0, [], "model_request_prepared"),
            ],
            expected_runtime,
            empty,
        )
        self.assertTrue(valid["valid"], valid)

        description_drift = copy.deepcopy(definition)
        description_drift["description"] = "不同描述"
        drift = catalog_identity_audit(
            [
                catalog(1, 0, [definition], "model_request_prepared"),
                catalog(
                    1,
                    1,
                    [description_drift],
                    "model_request_failed.retry.prepared",
                ),
            ],
            expected_runtime,
            empty,
        )
        self.assertFalse(drift["valid"])
        self.assertIn("catalog_retry_drift", drift["reasons"])

        orphan_retry = catalog_identity_audit(
            [
                {
                    **catalog(
                        1,
                        0,
                        [definition],
                        "model_request_prepared",
                    ),
                    "prior_attempt_id_sha256": canonical_hash("orphan"),
                }
            ],
            expected_runtime,
            empty,
        )
        self.assertFalse(orphan_retry["valid"])
        self.assertIn(
            "catalog_retry_attempt_lineage", orphan_retry["reasons"]
        )

        request_gap = catalog_identity_audit(
            [catalog(2, 0, [definition], "model_request_prepared")],
            expected_runtime,
            empty,
        )
        self.assertFalse(request_gap["valid"])
        self.assertIn(
            "catalog_request_number_sequence", request_gap["reasons"]
        )

        early_empty = catalog_identity_audit(
            [
                catalog(1, 0, [], "model_request_prepared"),
                catalog(2, 0, [definition], "model_request_prepared"),
            ],
            expected_runtime,
            empty,
        )
        self.assertFalse(early_empty["valid"])
        self.assertIn("terminal_empty_catalog_not_last", early_empty["reasons"])

        self.assertTrue(
            cancel_was_accepted(
                {"kind": "accepted", "run_id": "run-1"},
                "run-1",
            )
        )
        self.assertFalse(
            cancel_was_accepted(
                {"kind": "run", "run_id": "run-1"},
                "run-1",
            )
        )
        self.assertTrue(
            cancel_requires_canonical_get(
                CANARY.Failure("run_api_run_not_active")
            )
        )
        self.assertTrue(
            cancel_requires_canonical_get(
                CANARY.Failure("run_api_run_terminal")
            )
        )
        self.assertFalse(
            cancel_requires_canonical_get(
                CANARY.Failure("run_api_run_store_failed")
            )
        )
        terminal_run = {"terminal": {"state": "timed_out"}}
        self.assertIsNone(
            cancel_conflict_failure(
                CANARY.Failure("run_api_run_terminal"),
                terminal_run,
            )
        )
        self.assertIsNone(
            cancel_conflict_failure(
                CANARY.Failure("run_api_run_not_active"),
                terminal_run,
            )
        )
        self.assertEqual(
            cancel_conflict_failure(
                CANARY.Failure("run_api_run_not_active"),
                {"terminal": None},
            ),
            "run_inactive_without_terminal",
        )

    def test_inline_verification_artifact_is_exact(self) -> None:
        revision = {"status": "known", "sha256": "sha256:" + "a" * 64}
        content = {
            "summary": "冻结 verifier 通过",
            "verifier": verifier_spec("t1"),
            "verdict": "passed",
            "workspace_revision": revision,
        }
        encoded = canonical_bytes(content)
        digest = sha256_bytes(encoded)
        artifact_id = f"verification-evidence:{digest}"
        outcome = {
            "verifier_observation": {"artifact_ids": [artifact_id]},
            "artifacts": [
                {
                    "id": artifact_id,
                    "status": "available",
                    "sha256": digest,
                    "media_type": (
                        "application/vnd.codewhale.verification+json"
                    ),
                    "byte_len": len(encoded),
                    "inline_content": content,
                }
            ],
        }
        self.assertTrue(
            verification_artifacts_valid(
                outcome, verifier_spec("t1"), "passed", revision
            )
        )
        outcome["artifacts"][0]["inline_content"]["verdict"] = "failed"
        self.assertFalse(
            verification_artifacts_valid(
                outcome, verifier_spec("t1"), "passed", revision
            )
        )

    def test_host_receipt_advances_generation_without_revision_drift(self) -> None:
        revision = {"status": "known", "sha256": "sha256:" + "a" * 64}
        before = {"generation": 4, "revision": revision}
        after = {"generation": 5, "revision": revision}
        content = {
            "summary": "冻结 verifier 通过",
            "verifier": verifier_spec("t1"),
            "verdict": "passed",
            "workspace_revision": revision,
        }
        encoded = canonical_bytes(content)
        digest = sha256_bytes(encoded)
        artifact_id = f"verification-evidence:{digest}"
        outcome = {
            "invocation": "accepted",
            "transport": "succeeded",
            "operation": "succeeded",
            "side_effect": "not_applied",
            "workspace_revision": revision["sha256"],
            "evidence": {"status": "produced", "references": [artifact_id]},
            "verifier_observation": {
                "spec": verifier_spec("t1"),
                "verdict": "passed",
                "workspace_revision": revision,
                "artifact_ids": [artifact_id],
            },
            "artifacts": [
                {
                    "id": artifact_id,
                    "status": "available",
                    "sha256": digest,
                    "media_type": (
                        "application/vnd.codewhale.verification+json"
                    ),
                    "byte_len": len(encoded),
                    "inline_content": content,
                }
            ],
        }
        receipt = {
            "id": "receipt:verify-final",
            "generation_id": "generation-1",
            "acceptance_id": "m6b-t1",
            "verification_id": "verify-final",
            "verifier": verifier_spec("t1"),
            "workspace_state": after,
            "artifact_ids": [artifact_id],
            "lineage": {"policy": "latest_pass"},
        }
        events = [
            {
                "sequence": 1,
                "event": {
                    "kind": "run_created",
                    "request": {
                        "task_contract": {
                            "generation_id": "generation-1",
                            "definition": task_definition("t1"),
                        }
                    },
                },
            },
            {
                "sequence": 2,
                "event": {
                    "kind": "host_verification_prepared",
                    "verification_id": "verify-final",
                    "workspace_state_before": before,
                },
            },
            {
                "sequence": 3,
                "event": {
                    "kind": "host_verification_started",
                    "verification_id": "verify-final",
                },
            },
            {
                "sequence": 4,
                "event": {
                    "kind": "host_verification_committed",
                    "verification_id": "verify-final",
                    "outcome": outcome,
                    "receipt": receipt,
                    "workspace_state_after": after,
                },
            },
            {
                "sequence": 5,
                "event": {
                    "kind": "terminal",
                    "outcome": {
                        "terminal": {
                            "state": "completed",
                            "decision": {
                                "generation_id": "generation-1",
                                "workspace_state": after,
                                "satisfied": [
                                    {
                                        "kind": "evidence",
                                        "acceptance_id": "m6b-t1",
                                        "receipt_id": "receipt:verify-final",
                                    }
                                ],
                            },
                        }
                    },
                },
            },
        ]
        self.assertTrue(
            completion_receipt_summary("t1", events, "completed")["valid"]
        )
        forged = copy.deepcopy(events)
        forged[1]["event"]["workspace_state_before"] = after
        self.assertFalse(
            completion_receipt_summary("t1", forged, "completed")["valid"]
        )

    def test_t3_local_lineage_requires_real_started_operations(self) -> None:
        failed_state = {
            "generation": 0,
            "revision": {"status": "known", "sha256": "sha256:" + "a" * 64},
        }
        changed_state = {
            "generation": 1,
            "revision": {"status": "known", "sha256": "sha256:" + "b" * 64},
        }
        final_state = {"generation": 2, "revision": changed_state["revision"]}
        content = {
            "summary": "冻结 verifier 失败",
            "verifier": verifier_spec("t3"),
            "verdict": "failed",
            "workspace_revision": failed_state["revision"],
        }
        encoded = canonical_bytes(content)
        digest = sha256_bytes(encoded)
        artifact_id = f"verification-evidence:{digest}"
        failed_outcome = {
            "invocation": "accepted",
            "transport": "succeeded",
            "operation": "failed",
            "side_effect": "not_applied",
            "workspace_revision": failed_state["revision"]["sha256"],
            "evidence": {"status": "produced", "references": [artifact_id]},
            "verifier_observation": {
                "spec": verifier_spec("t3"),
                "verdict": "failed",
                "workspace_revision": failed_state["revision"],
                "artifact_ids": [artifact_id],
            },
            "artifacts": [
                {
                    "id": artifact_id,
                    "status": "available",
                    "sha256": digest,
                    "media_type": (
                        "application/vnd.codewhale.verification+json"
                    ),
                    "byte_len": len(encoded),
                    "inline_content": content,
                }
            ],
        }
        receipt = {
            "id": "receipt:final",
            "workspace_state": final_state,
            "lineage": {
                "policy": "failed_write_pass",
                "failure": {
                    "source": {"kind": "tool", "operation_id": "verify-before"},
                    "workspace_state": failed_state,
                    "artifact_ids": [artifact_id],
                },
                "mutation": {
                    "operation_id": "write",
                    "workspace_state_before": failed_state,
                    "workspace_state_after": changed_state,
                },
            },
        }
        events = [
            {
                "sequence": 1,
                "event": {
                    "kind": "tool_prepared",
                    "operation_id": "verify-before",
                    "workspace_access": "may_write",
                    "invocation": {
                        "name": "run_verifiers",
                        "arguments": {
                            "parsed": {"verifier_id": "m6b-t3"}
                        },
                    },
                },
            },
            {
                "sequence": 2,
                "event": {
                    "kind": "tool_execution_started",
                    "operation_id": "verify-before",
                },
            },
            {
                "sequence": 3,
                "event": {
                    "kind": "tool_outcome_committed",
                    "operation_id": "verify-before",
                    "name": "run_verifiers",
                    "outcome": failed_outcome,
                    "workspace_state": failed_state,
                },
            },
            {
                "sequence": 4,
                "event": {
                    "kind": "tool_prepared",
                    "operation_id": "write",
                    "workspace_access": "may_write",
                    "invocation": {
                        "name": "edit_file",
                        "arguments": {"parsed": {"path": "retry_window.py"}},
                    },
                },
            },
            {
                "sequence": 5,
                "event": {
                    "kind": "tool_execution_started",
                    "operation_id": "write",
                },
            },
            {
                "sequence": 6,
                "event": {
                    "kind": "tool_outcome_committed",
                    "operation_id": "write",
                    "name": "edit_file",
                    "outcome": {
                        "invocation": "accepted",
                        "transport": "succeeded",
                        "operation": "succeeded",
                        "side_effect": "applied",
                    },
                    "workspace_state": changed_state,
                },
            },
            {
                "sequence": 7,
                "event": {
                    "kind": "host_verification_committed",
                    "receipt": receipt,
                },
            },
        ]
        self.assertTrue(local_temporal_lineage_audit(events)["valid"])
        missing_started = [
            event
            for event in copy.deepcopy(events)
            if event_kind(event) != "tool_execution_started"
            or event["event"].get("operation_id") != "write"
        ]
        self.assertFalse(
            local_temporal_lineage_audit(missing_started)["valid"]
        )
        forged_arguments = copy.deepcopy(events)
        forged_arguments[0]["event"]["invocation"]["arguments"][
            "parsed"
        ] = {"profile": "exact"}
        self.assertFalse(
            local_temporal_lineage_audit(forged_arguments)["valid"]
        )
        missing_evidence = copy.deepcopy(events)
        missing_evidence[2]["event"]["outcome"].pop("evidence")
        self.assertFalse(
            local_temporal_lineage_audit(missing_evidence)["valid"]
        )
        forged_observation_revision = copy.deepcopy(events)
        forged_observation_revision[2]["event"]["outcome"][
            "verifier_observation"
        ]["workspace_revision"] = changed_state["revision"]
        self.assertFalse(
            local_temporal_lineage_audit(forged_observation_revision)["valid"]
        )

    def test_t3_delegated_lineage_settles_the_agent_operation(self) -> None:
        before = {
            "generation": 1,
            "revision": {"status": "known", "sha256": "sha256:" + "a" * 64},
        }
        after = {
            "generation": 2,
            "revision": {"status": "known", "sha256": "sha256:" + "b" * 64},
        }
        final = {"generation": 3, "revision": after["revision"]}
        child_receipt = {
            "id": "receipt:child-final",
            "lineage": {"policy": "failed_write_pass"},
        }
        root_receipt = {
            "id": "receipt:root-final",
            "workspace_state": final,
            "lineage": {
                "policy": "delegated_failed_write_pass",
                "child_run_id": "child-run",
                "child_receipt_id": child_receipt["id"],
                "integration": {
                    "operation_id": "integration",
                    "workspace_state_before": before,
                    "workspace_state_after": after,
                },
            },
        }
        task = {
            "task_id": "writer-task",
            "call_id": "agent-call",
            "child_run_id": "child-run",
        }
        root_events = [
            {
                "sequence": 1,
                "event": {
                    "kind": "tool_prepared",
                    "operation_id": "agent-operation",
                    "invocation": {"name": "agent", "call_id": "agent-call"},
                },
            },
            {
                "sequence": 2,
                "event": {
                    "kind": "tool_execution_started",
                    "operation_id": "agent-operation",
                },
            },
            {"sequence": 3, "event": {"kind": "agent_task_prepared", "task": task}},
            {
                "sequence": 4,
                "event": {
                    "kind": "child_started",
                    "child_run_id": "child-run",
                },
            },
            {"sequence": 5, "event": {"kind": "agent_seal_prepared"}},
            {"sequence": 6, "event": {"kind": "agent_seal_committed"}},
            {
                "sequence": 7,
                "event": {
                    "kind": "agent_result_collected",
                    "outcome": {"details": {"evidence": [child_receipt]}},
                },
            },
            {
                "sequence": 8,
                "event": {
                    "kind": "agent_integration_prepared",
                    "integration_id": "integration",
                    "expected_root_workspace_state": before,
                },
            },
            {
                "sequence": 9,
                "event": {
                    "kind": "agent_integration_started",
                    "integration_id": "integration",
                },
            },
            {
                "sequence": 10,
                "event": {
                    "kind": "agent_integration_committed",
                    "integration_id": "integration",
                    "root_workspace_state_after": after,
                },
            },
            {
                "sequence": 11,
                "event": {
                    "kind": "tool_outcome_committed",
                    "operation_id": "agent-operation",
                    "call_id": "agent-call",
                    "name": "agent",
                    "workspace_state": after,
                    "outcome": {
                        "invocation": "accepted",
                        "transport": "succeeded",
                        "operation": "succeeded",
                        "side_effect": "applied",
                    },
                },
            },
            {
                "sequence": 12,
                "event": {
                    "kind": "host_verification_committed",
                    "receipt": root_receipt,
                },
            },
        ]
        child_events = [
            {
                "sequence": 1,
                "event": {
                    "kind": "run_created",
                    "request": {"run_id": "child-run"},
                },
            },
            {
                "sequence": 2,
                "event": {
                    "kind": "host_verification_committed",
                    "receipt": child_receipt,
                },
            },
        ]
        with mock.patch(
            f"{__name__}.local_temporal_lineage_audit",
            return_value={"valid": True},
        ):
            self.assertTrue(
                delegated_temporal_lineage_audit(root_events, child_events)[
                    "valid"
                ]
            )
            forged = copy.deepcopy(root_events)
            forged[10]["event"]["workspace_state"] = before
            self.assertFalse(
                delegated_temporal_lineage_audit(forged, child_events)["valid"]
            )
            impossible_order = copy.deepcopy(root_events)
            impossible_order[0]["sequence"] = 4
            self.assertFalse(
                delegated_temporal_lineage_audit(
                    impossible_order, child_events
                )["valid"]
            )

    def test_writer_cleanup_plan_and_result_are_typed(self) -> None:
        task_id = "task-1"
        task = {
            "task_id": task_id,
            "workspace": {
                "allowed_paths": ["retry_window.py"],
                "base_commit": "a" * 40,
            }
        }
        seal = {
            "final_commit": "b" * 40,
            "diff_sha256": "c" * 64,
            "changed_files": ["retry_window.py"],
        }
        plan = {
            "phase": "post_integration",
            "reason_code": "writer_integrated",
            "ownership": {"state": "known", "identity_sha256": "d" * 64},
            "artifact_state": {
                "state": "known_host_sealed",
                "final_commit": seal["final_commit"],
                "diff_sha256": seal["diff_sha256"],
            },
            "scope": {
                "state": "known",
                "workspace_revision": {
                    "status": "known",
                    "sha256": "e" * 64,
                },
                "changed_count": 1,
                "in_scope_count": 1,
                "out_of_scope_count": 0,
                "path_set_sha256": writer_path_set_sha256(
                    ["retry_window.py"]
                ),
            },
            "mode": {
                "mode": "remove_exact",
                "expected_branch_commit": seal["final_commit"],
            },
        }

        def events(
            result: dict[str, Any],
            terminal: dict[str, Any] | None = None,
            context: str = "integrated",
        ) -> list[dict[str, Any]]:
            if context == "integrated":
                values = [
                    {
                        "sequence": 1,
                        "event": {"kind": "agent_integration_committed"},
                    }
                ]
            elif context == "seal":
                values = [
                    {
                        "sequence": 1,
                        "event": {"kind": "child_started"},
                    },
                    {
                        "sequence": 2,
                        "event": {"kind": "agent_seal_prepared"},
                    },
                ]
            else:
                values = [
                    {
                        "sequence": 1,
                        "event": {"kind": "child_started"},
                    },
                    {
                        "sequence": 2,
                        "event": {
                            "kind": "agent_result_collected",
                            "outcome": {
                                "terminal": {"state": "failed"}
                            },
                        },
                    },
                ]
            sequence = len(values) + 1
            values.extend([
                {
                    "sequence": sequence,
                    "event": {
                        "kind": "agent_cleanup_prepared",
                        "task_id": task_id,
                        "plan": copy.deepcopy(plan),
                    },
                },
                {
                    "sequence": sequence + 1,
                    "event": {
                        "kind": "agent_cleanup_committed",
                        "task_id": task_id,
                        "result": result,
                    },
                },
            ])
            if terminal is not None:
                values.append(
                    {
                        "sequence": sequence + 2,
                        "event": {
                            "kind": "terminal",
                            "outcome": {"terminal": terminal},
                        },
                    }
                )
            return values

        removed = writer_cleanup_summary(
            task_id,
            events(
                {
                    "status": "removed",
                    "worktree": "removed",
                    "branch": "already_absent",
                }
            ),
            task,
            seal,
            {"root_workspace_state_after": {}},
        )
        self.assertTrue(removed["identity_valid"], removed)
        self.assertTrue(removed["settled"])

        seal_failure_plan = copy.deepcopy(plan)
        seal_failure_plan["phase"] = "seal"
        seal_failure_plan["reason_code"] = "writer_seal_failed"
        seal_failure_plan["artifact_state"] = {"state": "known_unsealed"}
        seal_failure_plan["mode"]["expected_branch_commit"] = task[
            "workspace"
        ]["base_commit"]
        seal_failure_events = events(
            {"status": "already_absent"}, context="seal"
        )
        seal_failure_prepared = next(
            stored
            for stored in seal_failure_events
            if event_kind(stored) == "agent_cleanup_prepared"
        )
        seal_failure_prepared["event"]["plan"] = seal_failure_plan
        seal_failure = writer_cleanup_summary(
            task_id,
            seal_failure_events,
            task,
            {},
            {},
        )
        self.assertTrue(seal_failure["identity_valid"], seal_failure)
        self.assertTrue(seal_failure["settled"])

        uncertainty = "git_worktree_remove_failed"
        retained = writer_cleanup_summary(
            task_id,
            events(
                {
                    "status": "retained",
                    "worktree": "retained",
                    "branch": "removed",
                    "metadata": "clear",
                    "uncertainty_code": uncertainty,
                },
                {
                    "state": "recovery_required",
                    "ambiguity": {
                        "phase": "child_run",
                        "action_id": f"agent-cleanup:{task_id}",
                        "message": uncertainty,
                    },
                },
            ),
            task,
            seal,
            {"root_workspace_state_after": {}},
        )
        self.assertTrue(retained["identity_valid"], retained)
        self.assertTrue(retained["retained"])

        uncertainty = "writer_inspection_failed"
        uncertain_plan = copy.deepcopy(plan)
        uncertain_plan["phase"] = "child"
        uncertain_plan["reason_code"] = "writer_child_failed"
        uncertain_plan["ownership"] = {
            "state": "unknown",
            "uncertainty_code": uncertainty,
        }
        uncertain_plan["artifact_state"] = {"state": "unknown"}
        uncertain_plan["scope"] = {
            "state": "unknown",
            "uncertainty_code": uncertainty,
        }
        uncertain_plan["mode"] = {
            "mode": "retain_for_recovery",
            "uncertainty_code": uncertainty,
        }
        uncertain_events = events(
            {
                "status": "retained",
                "worktree": "retained",
                "branch": "retained",
                "metadata": "clear",
                "uncertainty_code": uncertainty,
            },
            {
                "state": "recovery_required",
                "ambiguity": {
                    "phase": "child_run",
                    "action_id": f"agent-cleanup:{task_id}",
                    "message": uncertainty,
                },
            },
            context="child",
        )
        cleanup_prepared = next(
            stored
            for stored in uncertain_events
            if event_kind(stored) == "agent_cleanup_prepared"
        )
        cleanup_prepared["event"]["plan"] = uncertain_plan
        uncertain = writer_cleanup_summary(
            task_id,
            uncertain_events,
            task,
            {},
            {},
        )
        self.assertTrue(uncertain["identity_valid"], uncertain)
        self.assertTrue(uncertain["retained"])

        malformed_plan_events = events({"status": "already_absent"})
        malformed_prepared = next(
            stored
            for stored in malformed_plan_events
            if event_kind(stored) == "agent_cleanup_prepared"
        )
        malformed_prepared["event"]["plan"]["scope"][
            "path_set_sha256"
        ] = "sha256:wrong"
        malformed = writer_cleanup_summary(
            task_id,
            malformed_plan_events,
            task,
            seal,
            {"root_workspace_state_after": {}},
        )
        self.assertFalse(malformed["identity_valid"])
        self.assertIn(
            "writer_cleanup_scope_hash", malformed["identity_reasons"]
        )

    def test_state_schema_is_read_only_and_exact(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            connection = sqlite3.connect(root / "state.db")
            connection.execute(f"PRAGMA user_version = {STATE_SCHEMA}")
            connection.close()
            self.assertTrue(state_schema_summary(root)["valid"])
            connection = sqlite3.connect(root / "state.db")
            connection.execute(f"PRAGMA user_version = {STATE_SCHEMA - 1}")
            connection.close()
            self.assertFalse(state_schema_summary(root)["valid"])

    def test_hard_stop_distinguishes_product_failure_from_bad_measurement(self) -> None:
        product_failure = {
            "measurement_valid": True,
            "product_outcome_observed": True,
            "verified_success": False,
            "false_success": False,
            "treatment": "writer",
            "path_scope_valid": True,
            "git": {"leak_free": True},
            "writer": {
                "integration_failures": 0,
                "event_counts": {},
                "cleanup": {"retained": False},
            },
            "treatment_audit": {"root_direct_write_violation": False},
        }
        self.assertEqual(arm_hard_stop_reasons(product_failure), [])
        writer_root_write = copy.deepcopy(product_failure)
        writer_root_write["treatment_audit"][
            "root_direct_write_violation"
        ] = True
        self.assertEqual(
            arm_hard_stop_reasons(writer_root_write),
            ["writer_root_direct_write"],
        )
        single_root_write = copy.deepcopy(writer_root_write)
        single_root_write["treatment"] = "single"
        self.assertEqual(arm_hard_stop_reasons(single_root_write), [])
        bad_measurement = {
            **product_failure,
            "measurement_valid": False,
            "measurement_invalid_reasons": ["catalog_definition_hash"],
            "false_success": True,
        }
        self.assertEqual(
            arm_hard_stop_reasons(bad_measurement),
            ["catalog_definition_hash"],
        )
        self.assertEqual(arm_hard_mechanism_reasons(bad_measurement), [])


def run_self_tests() -> int:
    suite = unittest.defaultTestLoader.loadTestsFromTestCase(HarnessSelfTests)
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    return 0 if result.wasSuccessful() else 1


def require_live_arguments(args: argparse.Namespace) -> None:
    if args.candidate_binary is None or args.candidate_revision is None:
        raise EvaluationError("candidate_binary_and_revision_required")
    if not args.acknowledge_cost or args.key_file is None:
        raise EvaluationError("live_run_requires_key_and_cost_acknowledgement")
    if args.runs_per_cell != RUNS_PER_CELL:
        raise EvaluationError("runs_per_cell_is_frozen_at_six")


def live(args: argparse.Namespace) -> dict[str, Any]:
    require_live_arguments(args)
    validate_frozen_hashes()
    validate_repository_identity(args.candidate_revision)
    candidate = binary_identity(
        args.candidate_binary, args.candidate_revision
    )
    if candidate["build_revision_bound"] is not True:
        raise EvaluationError("candidate_build_revision_unbound")
    identity = freeze_identity(candidate)
    key = CANARY.read_key(args.key_file.expanduser())
    accepted_pairs: list[dict[str, Any]] = []
    invalid_attempts: list[dict[str, Any]] = []
    partial = args.output.with_suffix(args.output.suffix + ".partial")
    suite_started = time.monotonic()
    for planned in schedule(args.runs_per_cell):
        accepted = False
        for attempt_index in range(1, MAX_PAIR_ATTEMPTS + 1):
            pair = {
                "pair_id": canonical_hash(
                    [
                        planned["task_id"],
                        planned["pair_index"],
                        attempt_index,
                    ]
                ),
                "schedule_position": planned["schedule_position"],
                "task_id": planned["task_id"],
                "pair_index": planned["pair_index"],
                "attempt_index": attempt_index,
                "order": planned["order"],
                "arms": [],
            }
            for arm_position, treatment in enumerate(
                planned["order"], start=1
            ):
                try:
                    validate_live_identity(
                        identity,
                        args.candidate_binary,
                        candidate["revision"],
                    )
                except EvaluationError as error:
                    return finalize_result(
                        args,
                        key,
                        identity,
                        accepted_pairs,
                        [*invalid_attempts, pair],
                        suite_started,
                        "aborted_identity_changed",
                        {
                            "reason": error.code,
                            "task_id": planned["task_id"],
                            "pair_index": planned["pair_index"],
                            "arm_position": arm_position,
                        },
                    )
                execution = known_execution_metrics(
                    accepted_pairs, [*invalid_attempts, pair]
                )
                reserve_nanousd = int(
                    RESOURCES["max_known_cost_usd_per_arm"]
                    * 1_000_000_000
                )
                suite_limit_nanousd = int(
                    RESOURCES["suite_known_cost_usd"]
                    * 1_000_000_000
                )
                if (
                    execution["unknown_billing_arms"] > 0
                    or execution["known_cost_nanousd"] + reserve_nanousd
                    > suite_limit_nanousd
                ):
                    return finalize_result(
                        args,
                        key,
                        identity,
                        accepted_pairs,
                        [*invalid_attempts, pair],
                        suite_started,
                        "aborted_cost_limit",
                        {
                            "reason": "suite_known_cost_limit",
                            "known_cost_nanousd": execution[
                                "known_cost_nanousd"
                            ],
                            "unknown_billing_arms": execution[
                                "unknown_billing_arms"
                            ],
                            "next_arm_reserve_nanousd": reserve_nanousd,
                        },
                    )
                started = time.monotonic()
                execution_state = {"api_exposure": "none"}
                try:
                    arm = execute_arm(
                        args.candidate_binary.resolve(),
                        candidate["sha256"],
                        candidate["revision"],
                        key,
                        planned["task_id"],
                        treatment,
                        planned["pair_index"],
                        attempt_index,
                        planned["order"],
                        arm_position,
                        execution_state,
                    )
                except Exception as error:
                    code = (
                        error.code
                        if isinstance(error, EvaluationError)
                        else "harness_postprocess_error"
                    )
                    arm = failure_arm_record(
                        planned["task_id"],
                        treatment,
                        planned["pair_index"],
                        attempt_index,
                        planned["order"],
                        arm_position,
                        code,
                        execution_state["api_exposure"],
                        int((time.monotonic() - started) * 1000),
                    )
                    if "accounting" in execution_state:
                        arm["requests"] = execution_state["accounting"]
                    if "accounting_provenance" in execution_state:
                        arm["accounting_provenance"] = execution_state[
                            "accounting_provenance"
                        ]
                    arm["harness_cancel_sent_after_runtime_grace"] = (
                        execution_state["harness_cancel_sent"]
                    )
                    arm["harness_cancel_raced_with_runtime_terminal"] = (
                        execution_state["harness_cancel_raced"]
                    )
                pair["arms"].append(arm)
                progress = partial_payload(
                    identity,
                    accepted_pairs,
                    [*invalid_attempts, pair],
                )
                result_redacted(progress, key)
                atomic_write_json(partial, progress)
                post_execution = known_execution_metrics(
                    accepted_pairs, [*invalid_attempts, pair]
                )
                arm_known_cost = nonnegative_int_or_zero(
                    arm.get("requests", {}).get("cost_nanousd")
                )
                if arm.get("requests", {}).get("billing_unknown") is True:
                    pair["measurement_valid"] = False
                    pair["unknown_billing_stop"] = True
                    invalid_attempts.append(pair)
                    return finalize_result(
                        args,
                        key,
                        identity,
                        accepted_pairs,
                        invalid_attempts,
                        suite_started,
                        "aborted_unknown_billing",
                        {
                            "reason": "unknown_billing_is_unbounded",
                            "arm_known_cost_nanousd": arm_known_cost,
                            "known_cost_nanousd": post_execution[
                                "known_cost_nanousd"
                            ],
                        },
                    )
                hard_stop = arm_hard_stop_reasons(arm)
                if hard_stop:
                    hard_mechanism_reasons = arm_hard_mechanism_reasons(arm)
                    pair["measurement_valid"] = False
                    pair["hard_stop_reasons"] = hard_stop
                    invalid_attempts.append(pair)
                    return finalize_result(
                        args,
                        key,
                        identity,
                        accepted_pairs,
                        invalid_attempts,
                        suite_started,
                        "aborted_hard_gate",
                        {
                            "reason": hard_stop[0],
                            "reasons": hard_stop,
                            "hard_mechanism_failure": bool(
                                hard_mechanism_reasons
                            ),
                            "hard_mechanism_reasons": hard_mechanism_reasons,
                            "task_id": planned["task_id"],
                            "pair_index": planned["pair_index"],
                            "arm_position": arm_position,
                        },
                    )
                if (
                    arm_known_cost > reserve_nanousd
                    or post_execution["known_cost_nanousd"]
                    > suite_limit_nanousd
                ):
                    pair["measurement_valid"] = False
                    pair["cost_boundary_exceeded"] = True
                    invalid_attempts.append(pair)
                    return finalize_result(
                        args,
                        key,
                        identity,
                        accepted_pairs,
                        invalid_attempts,
                        suite_started,
                        "aborted_cost_limit",
                        {
                            "reason": (
                                "arm_known_cost_limit"
                                if arm_known_cost > reserve_nanousd
                                else "suite_known_cost_limit"
                            ),
                            "arm_known_cost_nanousd": arm_known_cost,
                            "arm_limit_nanousd": reserve_nanousd,
                            "known_cost_nanousd": post_execution[
                                "known_cost_nanousd"
                            ],
                        },
                    )

            pair.update(pair_measurement_classification(pair["arms"]))
            if pair["measurement_valid"]:
                pair["comparison"] = pair_summary(pair)
                accepted_pairs.append(pair)
                accepted = True
            else:
                invalid_attempts.append(pair)
            progress = partial_payload(
                identity, accepted_pairs, invalid_attempts
            )
            result_redacted(progress, key)
            atomic_write_json(partial, progress)
            if accepted:
                break
            return finalize_result(
                args,
                key,
                identity,
                accepted_pairs,
                invalid_attempts,
                suite_started,
                "aborted_hard_gate",
                {
                    "pair_id": pair["pair_id"],
                    "reason": "measurement_invalid_without_arm_stop",
                },
            )
    try:
        validate_live_identity(
            identity,
            args.candidate_binary,
            candidate["revision"],
        )
    except EvaluationError as error:
        return finalize_result(
            args,
            key,
            identity,
            accepted_pairs,
            invalid_attempts,
            suite_started,
            "aborted_identity_changed",
            {"reason": error.code, "phase": "finalization"},
        )
    return finalize_result(
        args,
        key,
        identity,
        accepted_pairs,
        invalid_attempts,
        suite_started,
        "formal_same_binary_treatment_ab",
    )


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__)
    mode = result.add_mutually_exclusive_group()
    mode.add_argument("--self-test", action="store_true")
    mode.add_argument("--dry-run", action="store_true")
    mode.add_argument("--freeze-hashes", action="store_true")
    result.add_argument("--acknowledge-cost", action="store_true")
    result.add_argument("--key-file", type=Path)
    result.add_argument("--candidate-binary", type=Path)
    result.add_argument("--candidate-revision")
    result.add_argument("--runs-per-cell", type=int, default=RUNS_PER_CELL)
    result.add_argument(
        "--output",
        type=Path,
        default=ROOT / "eval/results/m6-b1-writer-benefit-ab-v3.json",
    )
    return result


def main() -> int:
    args = parser().parse_args()
    if args.self_test:
        return run_self_tests()
    if args.freeze_hashes:
        print(
            json.dumps(
                manifest_freeze_fields(),
                ensure_ascii=False,
                indent=2,
                sort_keys=True,
            )
        )
        return 0
    if args.dry_run:
        plan = dry_plan(args.candidate_binary, args.candidate_revision)
        print(
            json.dumps(
                plan,
                ensure_ascii=False,
                sort_keys=True,
                separators=(",", ":"),
            )
        )
        return 0
    result = live(args)
    print(
        json.dumps(
            {
                "output": str(args.output),
                "accepted_pairs": len(result["accepted_pairs"]),
                "invalid_attempts": len(result["invalid_attempts"]),
                "known_cost_usd": result["execution"][
                    "known_cost_nanousd"
                ]
                / 1_000_000_000,
                "decision": result["aggregate"]["decision"],
                "m6_b2_admitted": result["aggregate"][
                    "m6_b2_admitted"
                ],
            },
            ensure_ascii=False,
            sort_keys=True,
            separators=(",", ":"),
        )
    )
    return 0 if result["mode"] == "formal_same_binary_treatment_ab" else 2


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except EvaluationError as error:
        print(f"evaluation_error:{error.code}", file=sys.stderr)
        raise SystemExit(2)
