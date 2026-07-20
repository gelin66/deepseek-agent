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


ROOT = Path(__file__).resolve().parents[1]
MANIFEST_PATH = ROOT / "eval/manifests/m6-b1-writer-benefit-ab-v1.json"
CANARY_PATH = ROOT / "scripts/eval-m6-writer-canary.py"
RESULT_SCHEMA = "codewhale.eval.m6-writer-benefit.v1"
RUN_API_SCHEMA = 7
RUNTIME_EVENT_SCHEMA = 10
MODEL = "deepseek-v4-flash"
TREATMENTS = ("single", "writer")
TASK_IDS = ("t1", "t2", "t3")
RUNS_PER_CELL = 6
MAX_PAIR_ATTEMPTS = 3
PYTHON = Path("/usr/bin/python3")
RESAMPLEABLE_HARNESS_FAILURE_CODES = {
    "app_server_exited",
    "app_server_launch_failed",
    "stdio_json_invalid",
    "stdio_missing",
    "stdio_timeout",
    "stdio_write_failed",
}
SECRET_BOUNDARY_FAILURE_CODES = {
    "key_in_fixture",
    "key_in_protocol",
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
ROOT_BASE_CATALOG = [
    "apply_patch",
    "edit_file",
    "git_diff",
    "git_status",
    "grep_files",
    "list_dir",
    "read_file",
    "run_verifiers",
]
WRITER_ROOT_CATALOG = ["agent", *ROOT_BASE_CATALOG]
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
        value.get("schema") != "codewhale.eval.m6-writer-benefit-plan.v1"
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


def catalogs_are_exact(
    catalogs: list[dict[str, Any]], expected_names: list[str]
) -> bool:
    if not catalogs:
        return False
    empty_positions = [
        index
        for index, catalog in enumerate(catalogs)
        if catalog["tool_names"] == []
    ]
    if len(empty_positions) > 1:
        return False
    if empty_positions and empty_positions[0] != len(catalogs) - 1:
        return False
    return all(
        catalog["tool_names"] in (expected_names, [])
        for catalog in catalogs
    )


def t3_recovery_audit(events: list[dict[str, Any]]) -> dict[str, Any]:
    expected_parameters = verifier_spec("t3")["parameters"]
    calls = []
    outcomes: dict[str, dict[str, Any]] = {}
    write_sequences = []
    for stored in events:
        sequence = stored.get("sequence")
        event = stored.get("event", {})
        if event.get("kind") == "tool_prepared":
            invocation = event.get("invocation", {})
            name = invocation.get("name")
            if name in {"apply_patch", "edit_file"}:
                write_sequences.append(sequence)
            if name == "run_verifiers":
                calls.append(
                    {
                        "call_id": invocation.get("call_id"),
                        "prepared_sequence": sequence,
                        "arguments": invocation.get("arguments", {}).get(
                            "parsed"
                        ),
                    }
                )
        if (
            event.get("kind") == "tool_outcome_committed"
            and event.get("name") == "run_verifiers"
        ):
            observation = event.get("outcome", {}).get(
                "verifier_observation", {}
            )
            outcomes[event.get("call_id")] = {
                "committed_sequence": sequence,
                "verdict": observation.get("verdict"),
                "verifier_id": observation.get("spec", {}).get(
                    "verifier_id"
                ),
                "parameters": observation.get("spec", {}).get(
                    "parameters"
                ),
            }
    ordered = [
        {
            **call,
            **outcomes.get(call["call_id"], {}),
        }
        for call in calls
    ]
    exact = bool(ordered) and all(
        item.get("arguments") == expected_parameters
        and item.get("verifier_id") == "run_verifiers"
        and item.get("parameters") == expected_parameters
        for item in ordered
    )
    failed_then_passed = (
        len(ordered) >= 2
        and ordered[0].get("verdict") == "failed"
        and ordered[-1].get("verdict") == "passed"
    )
    ordered_around_write = (
        bool(write_sequences)
        and ordered[0].get("committed_sequence") is not None
        and ordered[-1].get("prepared_sequence") is not None
        and ordered[0]["committed_sequence"] < min(write_sequences)
        and ordered[-1]["prepared_sequence"] > max(write_sequences)
    )
    return {
        "valid": exact and failed_then_passed and ordered_around_write,
        "call_count": len(ordered),
        "exact_frozen_parameters": exact,
        "failed_then_passed": failed_then_passed,
        "ordered_around_write": ordered_around_write,
        "verdicts": [item.get("verdict") for item in ordered],
        "parameters_sha256": canonical_hash(expected_parameters),
    }


def catalog_summary(
    events: list[dict[str, Any]], actor: str
) -> list[dict[str, Any]]:
    result = []
    for event in event_values(events, "model_request_prepared"):
        request = event.get("request", {})
        tools = request.get("tools", []) if isinstance(request, dict) else []
        definitions = [tool for tool in tools if isinstance(tool, dict)]
        result.append(
            {
                "actor": actor,
                "request_number": request.get("request_number"),
                "attempt": request.get("attempt"),
                "tool_names": [
                    tool.get("name")
                    for tool in definitions
                    if isinstance(tool.get("name"), str)
                ],
                "catalog_sha256": canonical_hash(definitions),
            }
        )
    return result


def model_failure_summary(events: list[dict[str, Any]]) -> list[dict[str, Any]]:
    failures = []
    for event in event_values(events, "model_request_failed"):
        failure = event.get("failure", {})
        retry = event.get("retry", {})
        failures.append(
            {
                "code": failure.get("code"),
                "category": failure.get("category"),
                "retryable": failure.get("retryable"),
                "actionable_output": failure.get("actionable_output"),
                "retry_decision": retry.get("decision"),
                "retry_stop_reason": retry.get("reason"),
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


def actor_usage(events: list[dict[str, Any]]) -> dict[str, int]:
    result = usage_zero()
    for event in event_values(events, "model_response_committed"):
        output = event.get("output", {})
        usage = output.get("usage", {}) if isinstance(output, dict) else {}
        if isinstance(usage, dict):
            add_usage(result, usage)
    return result


def accounting_summary(run: dict[str, Any]) -> dict[str, Any]:
    value = run.get("accounting", {})
    usage = value.get("usage", {}) if isinstance(value, dict) else {}
    root = value.get("root", {}) if isinstance(value, dict) else {}
    child = value.get("child", {}) if isinstance(value, dict) else {}
    cost_nanousd = value.get("cost_nanousd")
    cost_nanocny = value.get("cost_nanocny")
    exact_billing = (
        value.get("billing_unknown") is False
        and value.get("unpriced") is False
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
    for surface in value.get("surface_usage", []) if isinstance(value, dict) else []:
        if not isinstance(surface, dict):
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


def accounting_valid(accounting: dict[str, Any]) -> tuple[bool, list[str]]:
    reasons = []
    root = accounting["root"]
    child = accounting["child"]
    started = int(root.get("started") or 0) + int(child.get("started") or 0)
    completed = int(root.get("completed") or 0) + int(child.get("completed") or 0)
    in_flight = int(root.get("in_flight") or 0) + int(child.get("in_flight") or 0)
    checks = {
        "hard_request_limit": accounting["hard_request_limit"]
        == RESOURCES["max_physical_api_requests_per_arm"],
        "request_range": 1 <= started
        <= RESOURCES["max_physical_api_requests_per_arm"],
        "requests_closed": started == completed and in_flight == 0,
        "complete": accounting["complete"] is True,
        "usage_complete": accounting["usage_complete"] is True,
        "usage_present": accounting["usage_missing"] is False,
        "usage_not_incomplete": accounting["usage_incomplete"] is False,
        "billing_known": accounting["billing_unknown"] is False,
        "priced": accounting["unpriced"] is False,
        "no_records_after_seal": accounting["records_after_seal"] == 0,
        "transport_retry_limit": int(accounting["transport_retries"] or 0)
        <= RESOURCES["accepted_transport_retries_per_arm"],
        "runtime_retry_limit": int(accounting["runtime_retries"] or 0)
        <= RESOURCES["max_runtime_retries_per_arm"],
        "budget_exhaustion_semantics": (
            accounting["budget_exhausted"] is True
        )
        == (int(accounting["exhausted_denied"] or 0) > 0),
        "cost_available": isinstance(accounting["cost_nanousd"], int)
        and isinstance(accounting["cost_nanocny"], int),
        "standard_chat_only": bool(accounting["surface_usage"])
        and all(
            surface.get("surface") == "standard_chat"
            and surface.get("model") == MODEL
            for surface in accounting["surface_usage"]
        ),
    }
    reasons.extend(name for name, passed in checks.items() if not passed)
    return not reasons, reasons


def run_created(events: list[dict[str, Any]]) -> dict[str, Any]:
    values = event_values(events, "run_created")
    return values[0].get("request", {}) if len(values) == 1 else {}


def typed_event_terminal_state(
    events: list[dict[str, Any]]
) -> str | None:
    values = event_values(events, "terminal")
    if len(values) != 1:
        return None
    terminal = values[0].get("outcome", {}).get("terminal", {})
    return terminal.get("state") if isinstance(terminal, dict) else None


def completion_receipt_summary(
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
    if len(receipts) == 1:
        receipt = receipts[0]
        receipt_hash = canonical_hash(receipt)
        workspace_state = receipt.get("workspace_state")
        decision = terminal.get("decision", {}) if isinstance(terminal, dict) else {}
        satisfied = decision.get("satisfied", []) if isinstance(decision, dict) else []
        valid = (
            terminal_state == "completed"
            and len(satisfied) == 1
            and satisfied[0].get("kind") == "evidence"
            and satisfied[0].get("receipt_id") == receipt.get("id")
            and decision.get("workspace_state") == workspace_state
            and committed[-1].get("workspace_state_after") == workspace_state
        )
    return {
        "valid": valid,
        "receipt_count": len(receipts),
        "receipt_sha256": receipt_hash,
        "workspace_state": workspace_state,
        "completion_rejections": len(event_values(events, "completion_rejected")),
    }


def writer_lifecycle_summary(
    task_id: str,
    root_events: list[dict[str, Any]],
    child_events: list[dict[str, Any]],
    base_commit: str,
    root_receipt: dict[str, Any],
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
    cleanup = event_values(root_events, "agent_cleanup_committed")
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
    assignment_valid = (
        len(prepared) == 1
        and workspace.get("access") == "isolated_write"
        and workspace.get("base_commit") == base_commit
        and workspace.get("allowed_paths")
        == MANIFEST["tasks"][task_id]["allowed_paths"]
        and workspace.get("worktree_path") != workspace.get("root_workspace")
    )
    seal_event = seal[0] if len(seal) == 1 else {}
    integration_event = integration[0] if len(integration) == 1 else {}
    cleanup_event = cleanup[0] if len(cleanup) == 1 else {}
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
        child_events, child_terminal
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
        and cleanup_event.get("worktree_removed") is True
        and cleanup_event.get("branch_removed") is True
        and cleanup_event.get("retained_for_recovery") is False
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
        "cleanup": {
            "worktree_removed": cleanup_event.get("worktree_removed"),
            "branch_removed": cleanup_event.get("branch_removed"),
            "retained_for_recovery": cleanup_event.get("retained_for_recovery"),
            "has_reason": bool(cleanup_event.get("reason")),
        },
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
    if treatment == "single":
        catalog_valid = catalogs_are_exact(
            root_catalogs, ROOT_BASE_CATALOG
        )
        contract_valid = (
            catalog_valid
            and agent_count == 0
            and child_count == 0
            and not child_events
            and set(git["changed_files"])
            == set(MANIFEST["tasks"][task_id]["expected_changed_files"])
        )
    else:
        catalog_valid = catalogs_are_exact(
            root_catalogs, WRITER_ROOT_CATALOG
        ) and catalogs_are_exact(child_catalogs, WRITER_CHILD_TOOLS)
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
        "root_catalog_requests": len(root_catalogs),
        "child_catalog_requests": len(child_catalogs),
        "root_terminal_empty_catalog_requests": sum(
            catalog["tool_names"] == [] for catalog in root_catalogs
        ),
        "child_terminal_empty_catalog_requests": sum(
            catalog["tool_names"] == [] for catalog in child_catalogs
        ),
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
    reason = terminal.get("reason")
    return {
        "state": terminal.get("state"),
        "reason_sha256": canonical_hash(reason) if isinstance(reason, str) else None,
        "reason_code": (
            reason.split("：", 1)[0]
            if isinstance(reason, str) and "：" in reason
            else None
        ),
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
        "resample_eligible": (
            api_exposure == "none"
            and code in RESAMPLEABLE_HARNESS_FAILURE_CODES
        ),
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
    resample_eligible = (
        has_gap
        and not mixed
        and not has_unobserved_execution
        and all(
            arm.get("measurement_valid") is True
            or arm.get("resample_eligible") is True
            for arm in arms
        )
    )
    return {
        "measurement_valid": measurement_valid,
        "mixed_product_failure_and_measurement_gap": mixed,
        "unobserved_model_execution": has_unobserved_execution,
        "resample_eligible": resample_eligible,
    }


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
        harness_cancel_sent = False
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
                    not harness_cancel_sent
                    and now >= cancel_deadline
                ):
                    response = client.call(
                        query(
                            "cancel",
                            run_id,
                            f"m6b-cancel-{evaluation_id}",
                        ),
                        timeout_seconds=remaining_stdio_timeout(deadline),
                    )
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
            execution_state["accounting"] = accounting_summary(run)
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
                    child_run = response.get("run", {})
                    child_events = collect_events(
                        client,
                        child_ids[0],
                        f"m6b-child-events-{evaluation_id}",
                        deadline,
                    )
        except CANARY.Failure as error:
            raise EvaluationError(error.code) from error
        finally:
            if client is not None:
                client.close()
            CANARY.stop(
                process,
                timeout_seconds=max(0.0, deadline - time.monotonic()),
            )

        stderr = stderr_path.read_bytes() if stderr_path.is_file() else b""
        if secret in stderr:
            raise EvaluationError("key_in_stderr")
        if CANARY.tree_contains(workspace, secret):
            raise EvaluationError("key_in_fixture")
        if CANARY.tree_contains(state_root, secret):
            raise EvaluationError("key_in_state")

        terminal = typed_terminal_summary(run)
        accounting = accounting_summary(run)
        execution_state["api_exposure"] = "accounted"
        execution_state["accounting"] = accounting
        accounting_is_valid, accounting_reasons = accounting_valid(accounting)
        budget_terminal_valid = (
            int(accounting["exhausted_denied"] or 0) == 0
            or terminal["reason_code"]
            == "llm_api_request_budget_exhausted"
        )
        if not budget_terminal_valid:
            accounting_reasons.append("budget_terminal_attribution")
            accounting_is_valid = False
        root_actor_usage = actor_usage(root_events)
        child_actor_usage = actor_usage(child_events)
        actor_total = usage_zero()
        add_usage(actor_total, root_actor_usage)
        add_usage(actor_total, child_actor_usage)
        actor_usage_matches = actor_total == accounting["usage"]
        if not actor_usage_matches:
            accounting_reasons.append("actor_usage_mismatch")
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
        root_receipt = completion_receipt_summary(
            root_events, terminal["state"]
        )
        writer = writer_lifecycle_summary(
            task_id,
            root_events,
            child_events,
            base_commit,
            root_receipt,
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
            "call_count": 0,
            "exact_frozen_parameters": True,
            "failed_then_passed": True,
            "ordered_around_write": True,
            "verdicts": [],
            "parameters_sha256": canonical_hash(
                verifier_spec("t3")["parameters"]
            ),
        }
        if task_id == "t3":
            evidence_events = child_events if treatment == "writer" else root_events
            t3_recovery = t3_recovery_audit(evidence_events)

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
        measurement_valid = not measurement_invalid_reasons
        mixed_failure_gap = product_failure and not measurement_valid
        resample_eligible = (
            not measurement_valid
            and not mixed_failure_gap
            and all(
                reason
                in {
                    "usage_complete",
                    "usage_present",
                    "usage_not_incomplete",
                    "billing_known",
                    "cost_available",
                    "actor_usage_mismatch",
                }
                for reason in measurement_invalid_reasons
            )
        )
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
            "resample_eligible": resample_eligible,
            "verified_success": verified_success,
            "false_success": false_success,
            "task_success_before_measurement": task_success_before_measurement,
            "wall_time_contract_valid": wall_time_valid,
            "harness_cancel_sent_after_runtime_grace": harness_cancel_sent,
            "budget_terminal_attribution_valid": budget_terminal_valid,
            "terminal": terminal,
            "completion": root_receipt,
            "completion_rejections": {
                "root": len(event_values(root_events, "completion_rejected")),
                "child": len(event_values(child_events, "completion_rejected")),
            },
            "task_contract_valid": task_contract_valid,
            "scope_valid": scope_valid,
            "path_scope_valid": path_scope_valid,
            "t3_failure_then_recovery": t3_recovery,
            "exact_verifier": exact_verifier,
            "treatment_audit": treatment_result,
            "writer": writer,
            "git": git,
            "requests": accounting,
            "actor_usage": {
                "root": root_actor_usage,
                "child": child_actor_usage,
                "total_matches_terminal": actor_usage_matches,
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
                "root": model_failure_summary(root_events),
                "child": model_failure_summary(child_events),
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
    if (
        arm.get("treatment") != "writer"
        or arm.get("product_outcome_observed") is not True
    ):
        return []
    reasons = []
    audit = arm.get("treatment_audit", {})
    writer = arm.get("writer", {})
    git = arm.get("git", {})
    counts = writer.get("event_counts", {})
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
    return list(dict.fromkeys(reasons))


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
    reliability = (
        per_task_non_regression
        and writer_success - single_success >= 1
        and token_overhead is not None
        and token_overhead <= 0.25
        and cost_overhead is not None
        and cost_overhead <= 0.25
    )
    complex_medians = {
        metric: pair_metrics["t2_t3"][metric]["paired_relative_median"]
        for metric in ("wall_time_ms", "tokens", "cost_nanousd")
    }
    minimum_dual_success = MANIFEST["statistics"][
        "efficiency_benefit"
    ]["minimum_dual_success_pairs_per_complex_task"]
    complex_dual_success_sample = all(
        pair_metrics[task]["wall_time_ms"]["dual_verified_success_pairs"]
        >= minimum_dual_success
        for task in ("t2", "t3")
    )
    complex_improvement = any(
        value is not None and value <= -0.15
        for value in complex_medians.values()
    )
    complex_bounded = all(
        value is not None and value <= 0.20
        for value in complex_medians.values()
    )
    each_complex_bounded = all(
        pair_metrics[task][metric]["paired_relative_median"] is not None
        and pair_metrics[task][metric]["paired_relative_median"] <= 0.20
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
    t1_control = (
        t1_success_non_regression
        and pair_metrics["t1"]["wall_time_ms"][
            "dual_verified_success_pairs"
        ]
        >= MANIFEST["statistics"]["t1_control"][
            "minimum_dual_success_pairs"
        ]
        and t1_ratios["tokens"] is not None
        and t1_ratios["tokens"] <= 0.25
        and t1_ratios["cost"] is not None
        and t1_ratios["cost"] <= 0.25
        and t1_ratios["wall_time"] is not None
        and t1_ratios["wall_time"] <= 0.20
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
        and complex_success_gain >= 1
        and complex_token_overhead is not None
        and complex_token_overhead <= 0.25
        and complex_cost_overhead is not None
        and complex_cost_overhead <= 0.25
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
    writer_false_success = sum(
        arm.get("false_success") is True
        for arm in observed_arms
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
        for arm in observed_arms
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
        and writer_false_success == 0
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
        writer_false_success > 0
        or bool(harness_safety_failures)
        or bool(safety_findings)
        or not owner_gate
        or (exact_pairs and not per_task_non_regression)
    )
    if mechanism_gate_failed:
        decision = "reject_and_rework"
    elif not exact_pairs or not exact_cells:
        decision = "hold_mechanism"
    elif hard_gate and (reliability or efficiency) and t1_control:
        decision = "keep_default"
    elif (
        hard_gate
        and complex_benefit
        and t1_success_non_regression
        and not t1_control
    ):
        decision = "shrink_on_demand"
    else:
        decision = "hold_mechanism"
    product_eligible = exact_pairs and exact_cells and all(
        not attempt.get("mixed_product_failure_and_measurement_gap", False)
        for attempt in invalid_attempts
    )
    return {
        "product_metric_eligible": product_eligible,
        "hard_gate_met": hard_gate,
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
        int(arm.get("requests", {}).get("cost_nanousd") or 0)
        for arm in arms
    )
    reserve_nanousd = int(
        RESOURCES["max_known_cost_usd_per_arm"] * 1_000_000_000
    )
    return {
        "arm_attempts": len(arms),
        "known_cost_nanousd": known_cost_nanousd,
        "known_cost_is_lower_bound": unknown_billing_arms > 0,
        "reserved_unknown_exposure_nanousd": (
            unknown_billing_arms * reserve_nanousd
        ),
        "budget_exposure_nanousd": (
            known_cost_nanousd + unknown_billing_arms * reserve_nanousd
        ),
        "known_cost_nanocny": sum(
            int(arm.get("requests", {}).get("cost_nanocny") or 0)
            for arm in arms
        ),
        "physical_requests_started": sum(
            int(arm.get("requests", {}).get("root", {}).get("started") or 0)
            + int(arm.get("requests", {}).get("child", {}).get("started") or 0)
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
        "source_owners": source_owner_audit(),
        "source_owner_gate_met": (
            source_owner_audit() == expected_source_owners()
        ),
        "app_server_argv": [
            "<candidate_binary>",
            "--provider",
            "deepseek",
            "app-server",
            "--stdio",
            "--transport-max-retries",
            str(RESOURCES["transport_max_retries_per_request"]),
        ],
        "candidate": candidate,
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
    checks = {
        "manifest_content": frozen.get(
            "manifest_sha256_excluding_this_field"
        )
        == manifest_content_hash(),
        "fixtures": frozen.get("fixture_tree_sha256")
        == actual_fixtures,
        "tasks": frozen.get("task_definition_sha256") == actual_tasks,
        "verifier_files": frozen.get("verifier_file_sha256")
        == actual_verifier_files,
        "verifier_specs": frozen.get("verifier_spec_sha256")
        == actual_verifier_specs,
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
        aggregate_result["product_metric_eligible"] = False
        aggregate_result["hard_gate_met"] = False
        aggregate_result["m6_b2_admitted"] = False
        aggregate_result["decision"] = (
            "reject_and_rework"
            if aggregate_result["writer_false_success"]
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
                "Writer root write tools remain advertised because child policy "
                "can only narrow parent policy; any root invocation fails the arm"
            ),
            "transport_retry_boundary": (
                "app-server was configured with --transport-max-retries=1; "
                "accepted arms also require observed total retries <=1 inside "
                "the shared physical budget"
            ),
            "cost_boundary": (
                "suite gate applies to known cost plus a frozen per-arm reserve; "
                "unknown billing is retained as a lower-bound exposure"
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
        self.assertEqual(MAX_PAIR_ATTEMPTS, 3)
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
        self.assertEqual(ROOT_BASE_CATALOG, sorted(ROOT_BASE_CATALOG))
        self.assertEqual(WRITER_ROOT_CATALOG, sorted(WRITER_ROOT_CATALOG))
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
        self.assertFalse(possible["resample_eligible"])
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
        self.assertTrue(before_api["resample_eligible"])
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

    def test_non_exact_billing_reserves_unknown_exposure(self) -> None:
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

    def test_pair_failure_and_gap_is_never_resampled(self) -> None:
        observed_failure = {
            "measurement_valid": True,
            "product_outcome_observed": True,
            "task_success_before_measurement": False,
            "resample_eligible": False,
            "api_exposure": "accounted",
        }
        pre_api_gap = {
            "measurement_valid": False,
            "product_outcome_observed": False,
            "task_success_before_measurement": None,
            "resample_eligible": True,
            "api_exposure": "none",
        }
        classified = pair_measurement_classification(
            [observed_failure, pre_api_gap]
        )
        self.assertTrue(
            classified["mixed_product_failure_and_measurement_gap"]
        )
        self.assertFalse(classified["resample_eligible"])

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

    def test_writer_safety_matrix_catches_scope_and_integration(self) -> None:
        reasons = writer_safety_reasons(
            {
                "treatment": "writer",
                "product_outcome_observed": True,
                "false_success": False,
                "path_scope_valid": False,
                "treatment_audit": {
                    "root_direct_write_violation": False,
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

    def test_canonical_source_owners_are_singular(self) -> None:
        self.assertEqual(source_owner_audit(), expected_source_owners())

    def test_runtime_terminal_catalog_is_empty_only_at_the_end(self) -> None:
        expected = ["read_file", "run_verifiers"]
        self.assertTrue(
            catalogs_are_exact(
                [
                    {"tool_names": expected},
                    {"tool_names": expected},
                    {"tool_names": []},
                ],
                expected,
            )
        )
        self.assertFalse(
            catalogs_are_exact(
                [
                    {"tool_names": expected},
                    {"tool_names": []},
                    {"tool_names": expected},
                ],
                expected,
            )
        )
        self.assertFalse(
            catalogs_are_exact(
                [{"tool_names": ["read_file"]}],
                expected,
            )
        )
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
                    execution["budget_exposure_nanousd"]
                    + reserve_nanousd
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
                            "budget_exposure_nanousd": execution[
                                "budget_exposure_nanousd"
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
                except EvaluationError as error:
                    arm = failure_arm_record(
                        planned["task_id"],
                        treatment,
                        planned["pair_index"],
                        attempt_index,
                        planned["order"],
                        arm_position,
                        error.code,
                        execution_state["api_exposure"],
                        int((time.monotonic() - started) * 1000),
                    )
                    if "accounting" in execution_state:
                        arm["requests"] = execution_state["accounting"]
                    arm["harness_cancel_sent_after_runtime_grace"] = (
                        execution_state["harness_cancel_sent"]
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
                arm_known_cost = int(
                    arm.get("requests", {}).get("cost_nanousd") or 0
                )
                if (
                    arm_known_cost > reserve_nanousd
                    or post_execution["budget_exposure_nanousd"]
                    > suite_limit_nanousd
                ):
                    pair["measurement_valid"] = False
                    pair["resample_eligible"] = False
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
                            "budget_exposure_nanousd": post_execution[
                                "budget_exposure_nanousd"
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
            if not pair["resample_eligible"]:
                return finalize_result(
                    args,
                    key,
                    identity,
                    accepted_pairs,
                    invalid_attempts,
                    suite_started,
                    "aborted_non_resampleable_measurement_gap",
                    {
                        "pair_id": pair["pair_id"],
                        "reason": (
                            "product_failure_and_measurement_gap"
                            if pair[
                                "mixed_product_failure_and_measurement_gap"
                            ]
                            else "unobserved_model_execution"
                            if pair["unobserved_model_execution"]
                            else "invalid_not_resample_eligible"
                        ),
                    },
                )
        if not accepted:
            return finalize_result(
                args,
                key,
                identity,
                accepted_pairs,
                invalid_attempts,
                suite_started,
                "aborted_pair_attempts_exhausted",
                {
                    "task_id": planned["task_id"],
                    "pair_index": planned["pair_index"],
                    "reason": "pair_measurement_attempts_exhausted",
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
        default=ROOT / "eval/results/m6-b1-writer-benefit-ab.json",
    )
    return result


def main() -> int:
    args = parser().parse_args()
    if args.self_test:
        return run_self_tests()
    if args.freeze_hashes:
        print(
            json.dumps(
                freeze_identity(),
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
