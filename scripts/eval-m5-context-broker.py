#!/usr/bin/env python3
"""Paired, credentialed M5-B ContextBroker evaluation.

The evaluator exercises the canonical ``app-server --stdio`` Run API for both
the frozen v7 baseline and v8 candidate. Every pair creates one shared
nine-phase source history, then runs compaction off/on from that exact source.
Raw prompts, model output, reasoning, tools, transcript data, verifier output,
stderr, temporary paths, and credentials never enter the result artifact.
"""

from __future__ import annotations

import argparse
import ast
import copy
import hashlib
import importlib.util
import json
import os
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
import unittest
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
HELPER_PATH = ROOT / "scripts/eval-m5-completion-gate.py"
FIXTURE_ROOT = ROOT / "eval/fixtures/m5-context-broker"
VERIFIER = FIXTURE_ROOT / "verifier.py"
RESULT_SCHEMA = "codewhale.eval.m5-context-broker.v2"
MODEL = "deepseek-v4-flash"
RUNS_PER_CELL = 3
PHASES = 9
MAX_PAIR_ATTEMPTS = 4
MAX_API_REQUESTS = 8
MAX_MODEL_REQUESTS = 6
MAX_OUTPUT_TOKENS = 4_096
MAX_RUNTIME_SECONDS = 420
HOST_VERIFIER_TIMEOUT_MS = 600_000
ACCEPTANCE_ID = "m5b-context-broker-v1"
EXPECTED_EVENT_SCHEMA = {"baseline": 7, "candidate": 8}
MODEL_TOOLS = [
    "read_file",
    "list_dir",
    "grep_files",
    "apply_patch",
    "edit_file",
    "git_diff",
    "git_status",
]
USAGE_FIELDS = (
    "input_tokens",
    "output_tokens",
    "cache_hit_tokens",
    "cache_miss_tokens",
    "cache_write_tokens",
    "reasoning_tokens",
    "reasoning_replay_tokens",
)
SCENARIOS = {
    "task-a": {
        "fixture": FIXTURE_ROOT / "task-a",
        "target": "ranges.py",
        "topic": "Rust 原生编码 Agent 的上下文边界、事件真相与工具原子性",
        "final_objective": (
            "修复 ranges.py 中 coalesce_ranges 的分段缺陷，只修改 ranges.py。"
            "同时把此前 M5B-A-01 至 M5B-A-09 的九个标记值按编号顺序直接拼接，"
            "写成模块级字符串常量 CONTEXT_PROOF；当前任务不会重复提供这些值。"
        ),
        "constraints": [
            "只允许修改 ranges.py。",
            "保持函数签名与返回格式不变。",
            "CONTEXT_PROOF 必须是一个字符串字面量。",
        ],
        "non_goals": [
            "不要修改测试、README、Git 元数据或新增文件。",
            "不要增加依赖、兼容层或无关抽象。",
        ],
        "source_failure": False,
    },
    "task-b": {
        "fixture": FIXTURE_ROOT / "task-b",
        "target": "settings.py",
        "topic": "DeepSeek 专用多 Agent 的证据新鲜度、失败恢复与完成权限",
        "final_objective": (
            "修复 settings.py 中 merge_settings 的递归合并缺陷，只修改 settings.py。"
            "同时把此前 M5B-B-01 至 M5B-B-09 的九个标记值按编号顺序直接拼接，"
            "写成模块级字符串常量 CONTEXT_PROOF；当前任务不会重复提供这些值。"
            "此前 Host 验证失败是有效事实，最终必须以本次确定性验证为准。"
        ),
        "constraints": [
            "只允许修改 settings.py。",
            "递归合并不得修改任一输入对象。",
            "CONTEXT_PROOF 必须是一个字符串字面量。",
        ],
        "non_goals": [
            "不要修改测试、README、Git 元数据或新增文件。",
            "不要增加依赖、兼容层或无关抽象。",
        ],
        "source_failure": True,
    },
}


class EvaluationError(RuntimeError):
    """Fail-closed evaluator error."""


def load_helper() -> Any:
    spec = importlib.util.spec_from_file_location("codewhale_m5_eval_helper", HELPER_PATH)
    if spec is None or spec.loader is None:
        raise EvaluationError("helper_module_unavailable")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


HELPER = load_helper()
canonical_hash = HELPER.canonical_hash
file_hash = HELPER.file_hash
event_kind = HELPER.event_kind
terminal_state = HELPER.terminal_state


def marker_values(scenario: str) -> list[str]:
    return [
        hashlib.sha256(
            f"codewhale-m5b-v1|{scenario}|{index:02d}".encode("utf-8")
        ).hexdigest()[:10]
        for index in range(1, PHASES + 1)
    ]


def expected_proof(scenario: str) -> str:
    return "".join(marker_values(scenario))


def phase_objective(scenario: str, phase: int) -> str:
    label = "A" if scenario == "task-a" else "B"
    value = marker_values(scenario)[phase - 1]
    topic = SCENARIOS[scenario]["topic"]
    if scenario == "task-b" and phase == PHASES - 1:
        return (
            f"这是冻结长上下文任务的第 {phase}/{PHASES} 阶段。"
            f"请记住标记 M5B-{label}-{phase:02d} 的值是 `{value}`，后续任务会要求按编号恢复。"
            "现在只在 settings.py 中添加一个模块级字符串常量 "
            'CONTEXT_PROOF = "source-stage-incomplete"，不得修复 merge_settings，'
            "不得修改其他文件。请先读取目标文件，再使用编辑工具完成这一项受控中间变更；"
            "不要复述标记名称和值。"
        )
    if scenario == "task-b" and phase == PHASES:
        return (
            f"这是冻结长上下文任务的第 {phase}/{PHASES} 阶段。"
            f"请记住标记 M5B-{label}-{phase:02d} 的值是 `{value}`。"
            "本阶段不得读取、修改文件或调用工具；请直接用一句“阶段完成”提出完成，"
            "让 Host 对当前受控中间状态执行确定性验证。不要复述标记名称和值。"
        )
    return (
        f"这是冻结长上下文任务的第 {phase}/{PHASES} 阶段。"
        f"请记住标记 M5B-{label}-{phase:02d} 的值是 `{value}`，后续任务会要求按编号恢复。"
        "本阶段绝对不要读取、修改文件或调用任何工具，也不要复述标记名称和值。"
        f"请仅围绕“{topic}”输出 450 至 650 个简体中文字符的独立分析，"
        "说明一个真实取舍、一个失败反例和一个可验证结论；不要提前处理后续编码任务。"
    )


def host_task(objective: str) -> dict[str, Any]:
    return {
        "objective": objective,
        "constraints": [],
        "non_goals": [],
        "acceptance": [
            {
                "kind": "host",
                "id": "host",
                "description": "Host 接受模型本轮回复",
            }
        ],
    }


def verifier_spec(scenario: str) -> dict[str, Any]:
    python = str(Path(sys.executable).resolve())
    args = [
        "-I",
        "-B",
        str(VERIFIER.resolve()),
        ".",
    ]
    command = {
        "name": ACCEPTANCE_ID,
        "program": python,
        "args": args,
        "cwd": "",
    }
    return {
        "verifier_id": "run_verifiers",
        "parameters": {
            "commands": [command],
            "level": "quick",
            "max_python_files": 200,
            "profile": "exact",
        },
        "plan": {
            "steps": [
                {
                    "id": ACCEPTANCE_ID,
                    "program": python,
                    "args": args,
                    "cwd": "",
                    "env": {},
                    "timeout_ms": HOST_VERIFIER_TIMEOUT_MS,
                }
            ]
        },
    }


def verifier_task(
    scenario: str,
    objective: str,
    constraints: list[str] | None = None,
    non_goals: list[str] | None = None,
) -> dict[str, Any]:
    return {
        "objective": objective,
        "constraints": constraints or [],
        "non_goals": non_goals or [],
        "acceptance": [
            {
                "kind": "verifier",
                "id": ACCEPTANCE_ID,
                "description": "冻结行为、上下文证明和工作区边界全部通过",
                "verifier": verifier_spec(scenario),
            }
        ],
    }


def final_task(scenario: str) -> dict[str, Any]:
    definition = SCENARIOS[scenario]
    return verifier_task(
        scenario,
        str(definition["final_objective"]),
        list(definition["constraints"]),
        list(definition["non_goals"]),
    )


def task_model_message(task: dict[str, Any]) -> str:
    message = f"任务目标：\n{task['objective']}"
    for title, key in (("约束", "constraints"), ("非目标", "non_goals")):
        if task[key]:
            message += f"\n\n{title}："
            for value in task[key]:
                message += f"\n- {value}"
    message += "\n\n验收条件："
    for acceptance in task["acceptance"]:
        message += f"\n- {acceptance['description']}"
        if acceptance["kind"] == "verifier":
            message += "（Host 将使用 `run_verifiers` 做确定性验证）"
    return message


def common_run_fields(workspace: Path, model: str) -> dict[str, Any]:
    return {
        "workspace": str(workspace.resolve()),
        "model": model,
        "reasoning_effort": "high",
        "max_output_tokens": MAX_OUTPUT_TOKENS,
        "max_api_requests": MAX_API_REQUESTS,
        "streaming": True,
        "tool_policy": {
            "enabled": True,
            "allowed": MODEL_TOOLS,
            "denied": [],
        },
        "limits": {
            "max_turns": 24,
            "max_model_requests": MAX_MODEL_REQUESTS,
            "max_model_retries": 1,
            "max_tool_calls": 16,
            "max_depth": 0,
            "max_concurrent_children": 1,
            "model_event_idle_ms": 120_000,
            "wall_time_ms": MAX_RUNTIME_SECONDS * 1000,
        },
        "controls": {
            "auto_approve": True,
            "trust_mode": False,
            "allow_sandbox_elevation": False,
            "interactive": False,
            "sandbox": "workspace-write",
        },
    }


def start_command(workspace: Path, model: str, task: dict[str, Any]) -> dict[str, Any]:
    command = {"kind": "start", "task": task}
    command.update(common_run_fields(workspace, model))
    return command


def initialize_workspace(scenario: str, destination: Path) -> dict[str, Any]:
    HELPER.FIXTURE = Path(SCENARIOS[scenario]["fixture"])
    return HELPER.initialize_workspace(destination)


def restore_workspace(backup: Path, workspace: Path) -> None:
    if workspace.exists():
        shutil.rmtree(workspace)
    shutil.copytree(backup, workspace, symlinks=True)


def set_source_failure_workspace_read_only(workspace: Path, enabled: bool) -> None:
    target = workspace / str(SCENARIOS["task-b"]["target"])
    target.chmod(0o444 if enabled else 0o644)
    workspace.chmod(0o555 if enabled else 0o755)


def wait_terminal(
    server: Any, run: dict[str, Any], timeout_seconds: int = MAX_RUNTIME_SECONDS + 30
) -> dict[str, Any]:
    deadline = time.monotonic() + timeout_seconds
    while run.get("terminal") is None:
        if time.monotonic() >= deadline:
            raise EvaluationError("run_terminal_timeout")
        time.sleep(0.25)
        result = server.request({"kind": "get", "run_id": run["run_id"]})
        if result.get("kind") != "run" or not isinstance(result.get("run"), dict):
            raise EvaluationError("get_did_not_return_run")
        run = result["run"]
    return run


def events_for(server: Any, run_id: str) -> list[dict[str, Any]]:
    result = server.request(
        {"kind": "events", "run_id": run_id, "after_sequence": 0}
    )
    events = result.get("events")
    if result.get("kind") != "events" or not isinstance(events, list) or not events:
        raise EvaluationError("canonical_events_missing")
    terminal_state(events)
    return events


def submit_run(server: Any, command: dict[str, Any]) -> tuple[dict[str, Any], list[dict[str, Any]]]:
    result = server.request(command, timeout=60)
    run = result.get("run")
    if result.get("kind") != "run" or not isinstance(run, dict):
        raise EvaluationError("command_did_not_return_run")
    if not isinstance(run.get("run_id"), str):
        raise EvaluationError("run_id_missing")
    run = wait_terminal(server, run)
    return run, events_for(server, run["run_id"])


def run_created_request(events: list[dict[str, Any]]) -> dict[str, Any]:
    created = [
        stored["event"].get("request")
        for stored in events
        if event_kind(stored) == "run_created"
    ]
    if len(created) != 1 or not isinstance(created[0], dict):
        raise EvaluationError("run_created_request_missing")
    return created[0]


def first_model_request(events: list[dict[str, Any]]) -> dict[str, Any] | None:
    for stored in events:
        if event_kind(stored) == "model_request_prepared":
            request = stored["event"].get("request")
            if not isinstance(request, dict):
                raise EvaluationError("model_request_missing")
            return request
    return None


def event_schema_valid(events: list[dict[str, Any]], variant: str) -> bool:
    return {
        stored.get("schema_version")
        for stored in events
        if isinstance(stored, dict)
    } == {EXPECTED_EVENT_SCHEMA[variant]}


def terminal_name(events: list[dict[str, Any]]) -> str:
    state, _ = terminal_state(events)
    return state


def terminal_failure_kind(events: list[dict[str, Any]]) -> str | None:
    _, terminal = terminal_state(events)
    failure = terminal.get("failure")
    return failure.get("kind") if isinstance(failure, dict) else None


def terminal_accounting_projection_valid(
    terminal_outcome: Any, run: dict[str, Any]
) -> bool:
    terminal_accounting = (
        terminal_outcome.get("accounting")
        if isinstance(terminal_outcome, dict)
        else None
    )
    projected_accounting = run.get("accounting")
    if not isinstance(terminal_accounting, dict) or not isinstance(
        projected_accounting, dict
    ):
        return False
    excluded = {"sealed", "runtime_retries"}
    terminal_comparable = {
        key: value
        for key, value in terminal_accounting.items()
        if key not in excluded
    }
    projected_comparable = {
        key: value
        for key, value in projected_accounting.items()
        if key not in excluded
    }
    return all(
        (
            terminal_comparable == projected_comparable,
            terminal_accounting.get("sealed") is True,
            projected_accounting.get("sealed") is False,
            int(terminal_accounting.get("runtime_retries", 0))
            == int(projected_accounting.get("runtime_retries", 0))
            + int(run.get("runtime_retries", 0)),
            terminal_outcome.get("runtime_model_requests")
            == run.get("runtime_model_requests"),
            terminal_outcome.get("runtime_retries") == run.get("runtime_retries"),
            terminal_outcome.get("tool_calls") == run.get("tool_calls"),
        )
    )


def run_usage_projection_valid(run: dict[str, Any]) -> bool:
    accounting = run.get("accounting")
    physical = accounting.get("usage") if isinstance(accounting, dict) else None
    logical = run.get("usage")
    if not isinstance(physical, dict) or not isinstance(logical, dict):
        return False
    if set(physical) != set(logical):
        return False
    if not all(
        isinstance(physical[field], int)
        and not isinstance(physical[field], bool)
        and physical[field] >= 0
        and isinstance(logical[field], int)
        and not isinstance(logical[field], bool)
        and logical[field] >= 0
        and physical[field] >= logical[field]
        for field in physical
    ):
        return False
    retry_exercised = (
        int(run.get("runtime_retries", 0)) > 0
        or int(accounting.get("transport_retries", 0)) > 0
    )
    return retry_exercised or physical == logical


def nonnegative_integer(value: Any) -> bool:
    return isinstance(value, int) and not isinstance(value, bool) and value >= 0


def accounting_invariants_valid(accounting: dict[str, Any]) -> bool:
    root = accounting.get("root")
    child = accounting.get("child")
    if not isinstance(root, dict) or not isinstance(child, dict):
        return False
    actor_fields = ("started", "completed", "in_flight", "retries")
    top_level_counts = (
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
    if not all(
        nonnegative_integer(actor.get(field))
        for actor in (root, child)
        for field in actor_fields
    ) or not all(
        nonnegative_integer(accounting.get(field)) for field in top_level_counts
    ):
        return False
    actors_settled = all(
        all(
            (
                actor["in_flight"] == 0,
                actor["started"] == actor["completed"],
                actor["retries"] <= actor["started"],
            )
        )
        for actor in (root, child)
    )
    usage_missing = accounting["usage_missing_responses"] > 0
    usage_incomplete = accounting["incomplete_responses"] > 0
    billing_unknown = accounting["billing_unknown_attempts"] > 0
    unpriced = accounting["unpriced_usage_responses"] > 0
    records_after_seal = accounting["records_after_seal"] > 0
    completed = root["completed"] + child["completed"]
    usage_complete = not usage_missing and not usage_incomplete
    complete = (
        actors_settled
        and usage_complete
        and not billing_unknown
        and not unpriced
        and not records_after_seal
    )
    return all(
        (
            actors_settled,
            accounting.get("sealed") is False,
            accounting["sealed_denied"] == 0,
            accounting.get("usage_missing") is usage_missing,
            accounting.get("usage_incomplete") is usage_incomplete,
            accounting.get("billing_unknown") is billing_unknown,
            accounting.get("unpriced") is unpriced,
            accounting.get("usage_complete") is usage_complete,
            accounting.get("complete") is complete,
            accounting.get("budget_exhausted")
            is (accounting["exhausted_denied"] > 0),
            accounting["transport_retries"]
            == root["retries"] + child["retries"],
            accounting["usage_responses"] <= completed,
            accounting["usage_missing_responses"] <= completed,
            accounting["incomplete_responses"] <= completed,
            accounting["billing_unknown_attempts"] <= completed,
            accounting["unpriced_usage_responses"]
            <= accounting["usage_responses"],
        )
    )


def accounting_assessment(
    run: dict[str, Any], events: list[dict[str, Any]]
) -> tuple[dict[str, Any], dict[str, Any]]:
    HELPER.MAX_API_REQUESTS = MAX_API_REQUESTS
    accounting = run.get("accounting")
    physical_usage = accounting.get("usage") if isinstance(accounting, dict) else None
    projected = {**run, "usage": physical_usage}
    metrics = HELPER.accounting_metrics(projected)
    normalized = copy.deepcopy(projected)
    normalized_accounting = normalized["accounting"]
    normalized_accounting.update(
        {
            "complete": True,
            "usage_complete": True,
            "usage_missing": False,
            "usage_incomplete": False,
            "billing_unknown": False,
        }
    )
    normalized_observability_valid = HELPER.accounting_metrics(normalized)["valid"]
    terminal_state(events)
    terminal_outcome = next(
        stored["event"].get("outcome")
        for stored in events
        if event_kind(stored) == "terminal"
    )
    usage = {
        field: int(metrics["usage"].get(field, 0)) for field in USAGE_FIELDS
    }
    usage["total_tokens"] = usage["input_tokens"] + usage["output_tokens"]
    terminal_projection_valid = terminal_accounting_projection_valid(
        terminal_outcome, run
    )
    usage_projection_valid = run_usage_projection_valid(run)
    model_request_limit_valid = (
        int(run.get("runtime_model_requests", 0)) <= MAX_MODEL_REQUESTS
    )
    axes = {
        "helper_valid": bool(metrics["valid"]),
        "terminal_projection_valid": terminal_projection_valid,
        "usage_projection_valid": usage_projection_valid,
        "model_request_limit_valid": model_request_limit_valid,
        "hard_request_limit_valid": (
            accounting.get("hard_request_limit") == MAX_API_REQUESTS
        ),
        "api_request_limit_valid": (
            int(metrics["api_requests"]) <= MAX_API_REQUESTS
        ),
        "accounting_invariants_valid": accounting_invariants_valid(accounting),
        "normalized_observability_valid": normalized_observability_valid,
        "complete": accounting.get("complete") is True,
        "usage_complete": accounting.get("usage_complete") is True,
        "usage_missing_absent": accounting.get("usage_missing") is False,
        "usage_incomplete_absent": accounting.get("usage_incomplete") is False,
        "billing_known": accounting.get("billing_unknown") is False,
        "priced": accounting.get("unpriced") is False,
        "budget_not_exhausted": (
            accounting.get("budget_exhausted") is False
            and int(accounting.get("exhausted_denied", 0)) == 0
        ),
        "no_in_flight": (
            int(accounting.get("root", {}).get("in_flight", 0)) == 0
            and int(accounting.get("child", {}).get("in_flight", 0)) == 0
        ),
        "no_records_after_seal": (
            int(accounting.get("records_after_seal", 0)) == 0
        ),
    }
    metrics["valid"] = all(axes.values())
    metrics["usage"] = usage
    logical_usage = run.get("usage")
    usage_delta = {
        field: (
            int(physical_usage.get(field, 0)) - int(logical_usage.get(field, 0))
            if isinstance(physical_usage, dict) and isinstance(logical_usage, dict)
            else None
        )
        for field in USAGE_FIELDS
    }
    audit = {
        "valid": metrics["valid"],
        "api_started": int(metrics["api_requests"]),
        "runtime_model_requests": int(run.get("runtime_model_requests", 0)),
        "runtime_retries": int(run.get("runtime_retries", 0)),
        "transport_retries": int(accounting.get("transport_retries", 0)),
        "axes": axes,
        "physical_minus_logical": usage_delta,
    }
    return metrics, audit


def accounting_metrics(run: dict[str, Any], events: list[dict[str, Any]]) -> dict[str, Any]:
    metrics, _ = accounting_assessment(run, events)
    return metrics


def sum_metrics(values: list[dict[str, Any]]) -> dict[str, Any]:
    usage = {
        field: sum(int(value["usage"].get(field, 0)) for value in values)
        for field in (*USAGE_FIELDS, "total_tokens")
    }
    return {
        "valid": all(value["valid"] for value in values),
        "api_requests": sum(int(value["api_requests"]) for value in values),
        "runtime_model_requests": sum(
            int(value["runtime_model_requests"]) for value in values
        ),
        "runtime_retries": sum(int(value["runtime_retries"]) for value in values),
        "tool_calls": sum(int(value["tool_calls"]) for value in values),
        "usage": usage,
        "cost_usd": round(sum(float(value["cost_usd"]) for value in values), 9),
        "cost_cny": round(sum(float(value["cost_cny"]) for value in values), 9),
    }


def pair_attempt_metrics(pair: dict[str, Any]) -> dict[str, Any]:
    values = [pair["source"]["metrics"]]
    for arm in pair["arms"]:
        values.extend((arm["compact_metrics"], arm["final_metrics"]))
    return sum_metrics(values)


def measurement_execution_summary(
    accepted_pairs: list[dict[str, Any]],
    invalid_attempts: list[dict[str, Any]],
) -> dict[str, Any]:
    attempts = [*invalid_attempts, *accepted_pairs]
    known_minimum = sum_metrics(
        [pair["execution_metrics"] for pair in attempts]
    )
    return {
        "total_pair_attempts": len(attempts),
        "accepted_pairs": len(accepted_pairs),
        "invalid_pair_attempts": len(invalid_attempts),
        "unaccounted_pair_attempts": 0,
        "pair_attempt_duration_seconds": round(
            sum(float(pair["pair_duration_seconds"]) for pair in attempts), 3
        ),
        "invalid_attempt_duration_seconds": round(
            sum(
                float(pair["pair_duration_seconds"])
                for pair in invalid_attempts
            ),
            3,
        ),
        "accounting_complete": known_minimum["valid"],
        "cost_semantics": (
            "exact" if known_minimum["valid"] else "known_minimum"
        ),
        "known_minimum_metrics": known_minimum,
    }


def accounting_gap_resample_eligible(audit: dict[str, Any]) -> bool:
    if audit["valid"]:
        return False
    axes = audit["axes"]
    protected_axes = (
        "terminal_projection_valid",
        "usage_projection_valid",
        "model_request_limit_valid",
        "hard_request_limit_valid",
        "api_request_limit_valid",
        "accounting_invariants_valid",
        "normalized_observability_valid",
        "priced",
        "budget_not_exhausted",
        "no_in_flight",
        "no_records_after_seal",
    )
    observability_axes = (
        "complete",
        "usage_complete",
        "usage_missing_absent",
        "usage_incomplete_absent",
        "billing_known",
    )
    retries = int(audit["runtime_retries"]) + int(audit["transport_retries"])
    return all(axes[axis] for axis in protected_axes) and any(
        not axes[axis] for axis in observability_axes
    ) and retries > 0


def arm_resample_eligible(arm: dict[str, Any]) -> bool:
    failed_axes = {
        axis for axis, valid in arm["measurement_axes"].items() if not valid
    }
    accounting_axes = {"compact_accounting", "final_accounting"}
    if not failed_axes or not failed_axes.issubset(accounting_axes):
        return False
    audits = {
        "compact_accounting": arm["compact_accounting_audit"],
        "final_accounting": arm["final_accounting_audit"],
    }
    return all(
        accounting_gap_resample_eligible(audits[axis])
        for axis in failed_axes
    )


def pair_resample_eligible(
    arms: list[dict[str, Any]], pair_contract_valid: bool
) -> bool:
    return (
        pair_contract_valid
        and any(not arm["measurement_valid"] for arm in arms)
        and all(
            arm["measurement_valid"] or arm_resample_eligible(arm)
            for arm in arms
        )
    )


def run_external_verifier(scenario: str, workspace: Path) -> dict[str, Any]:
    del scenario
    HELPER.VERIFIER = VERIFIER
    return HELPER.run_external_verifier(workspace)


def validate_receipt(
    events: list[dict[str, Any]],
    run: dict[str, Any],
    scenario: str,
) -> dict[str, Any]:
    original_acceptance = HELPER.ACCEPTANCE_ID
    original_spec = HELPER.verifier_spec
    HELPER.ACCEPTANCE_ID = ACCEPTANCE_ID
    HELPER.verifier_spec = lambda: verifier_spec(scenario)
    try:
        gate = HELPER.validate_candidate_gate(events, run, True)
    finally:
        HELPER.ACCEPTANCE_ID = original_acceptance
        HELPER.verifier_spec = original_spec
    contract_valid = run.get("task_contract", {}).get("definition") == final_task(
        scenario
    )
    valid = gate["valid"] and gate["exercised"] and contract_valid
    return {
        "valid": valid,
        "gate_valid": gate["valid"],
        "gate_exercised": gate["exercised"],
        "contract_valid": contract_valid,
        "receipt_sha256": (
            canonical_hash(gate["audit"].get("receipt")) if valid else None
        ),
    }


def validate_expected_source_failure(
    events: list[dict[str, Any]], run: dict[str, Any], scenario: str
) -> bool:
    if scenario != "task-b" or terminal_name(events) != "blocked":
        return False
    kinds = [event_kind(stored) for stored in events]
    proposals = [
        stored["event"].get("candidate")
        for stored in events
        if event_kind(stored) == "completion_proposed"
    ]
    prepared = [
        stored["event"]
        for stored in events
        if event_kind(stored) == "host_verification_prepared"
    ]
    started = [
        stored["event"]
        for stored in events
        if event_kind(stored) == "host_verification_started"
    ]
    commits = [
        stored["event"]
        for stored in events
        if event_kind(stored) == "host_verification_committed"
    ]
    rejections = [
        stored["event"].get("rejection")
        for stored in events
        if event_kind(stored) == "completion_rejected"
    ]
    cycle_count = len(prepared)
    prepared_candidates = [item.get("candidate") for item in prepared]
    prepared_ids = [item.get("verification_id") for item in prepared]
    outcomes = [item.get("outcome") for item in commits]
    return all(
        (
            bool(proposals),
            cycle_count >= 1,
            len(started) == len(commits) == len(rejections) == cycle_count,
            all(item.get("acceptance_id") == ACCEPTANCE_ID for item in prepared),
            all(item.get("verifier") == verifier_spec(scenario) for item in prepared),
            all(
                isinstance(candidate, dict) and candidate in proposals
                for candidate in prepared_candidates
            ),
            all(
                verification_id
                == f"host-verification:{candidate.get('id')}:{ACCEPTANCE_ID}"
                for verification_id, candidate in zip(
                    prepared_ids, prepared_candidates
                )
                if isinstance(candidate, dict)
            ),
            [item.get("verification_id") for item in started] == prepared_ids,
            [item.get("verification_id") for item in commits] == prepared_ids,
            all(item.get("receipt") is None for item in commits),
            all(failed_verifier_outcome_valid(outcome) for outcome in outcomes),
            [
                item.get("candidate_id") if isinstance(item, dict) else None
                for item in rejections
            ]
            == [
                candidate.get("id") if isinstance(candidate, dict) else None
                for candidate in prepared_candidates
            ],
            all(
                isinstance(item, dict)
                and item.get("unmet_acceptance_ids") == [ACCEPTANCE_ID]
                for item in rejections
            ),
            run.get("task_contract", {}).get("definition", {}).get("acceptance", [{}])[
                0
            ].get("kind")
            == "verifier",
        )
    )


def failed_verifier_outcome_valid(outcome: Any) -> bool:
    return (
        isinstance(outcome, dict)
        and {
            key: outcome.get(key)
            for key in ("invocation", "transport", "operation", "side_effect", "retry")
        }
        == {
            "invocation": "accepted",
            "transport": "succeeded",
            "operation": "failed",
            "side_effect": "indeterminate",
            "retry": "unsafe",
        }
        and outcome.get("evidence") == {"status": "rejected"}
        and outcome.get("artifacts", []) == []
        and outcome.get("workspace_revision") is None
        and outcome.get("verifier_observation") is None
    )


def source_failure_fact_assessment(
    events: list[dict[str, Any]],
) -> tuple[dict[str, Any] | None, dict[str, Any]]:
    commits = [
        stored["event"]
        for stored in events
        if event_kind(stored) == "host_verification_committed"
    ]
    rejections = [
        stored["event"].get("rejection")
        for stored in events
        if event_kind(stored) == "completion_rejected"
    ]
    if not commits and not rejections:
        return None, {
            "valid": True,
            "commit_count": 0,
            "rejection_count": 0,
            "failed_outcome_valid": None,
            "rejection_valid": None,
            "receipt_absent": None,
        }
    presence_valid = bool(commits) and bool(rejections)
    if not presence_valid:
        return None, {
            "valid": False,
            "commit_count": len(commits),
            "rejection_count": len(rejections),
            "failed_outcome_valid": None,
            "rejection_valid": None,
            "receipt_absent": None,
        }
    commit = commits[-1]
    outcome = commit.get("outcome")
    rejection = rejections[-1]
    outcome_valid = failed_verifier_outcome_valid(outcome)
    rejection_valid = isinstance(rejection, dict)
    receipt_absent = commit.get("receipt") is None
    valid = outcome_valid and rejection_valid and receipt_absent
    audit = {
        "valid": valid,
        "commit_count": len(commits),
        "rejection_count": len(rejections),
        "failed_outcome_valid": outcome_valid,
        "rejection_valid": rejection_valid,
        "receipt_absent": receipt_absent,
    }
    if not valid:
        return None, audit
    return {
        "rejection": rejection,
        "outcome": outcome,
        "workspace_state": commit.get("workspace_state_after"),
    }, audit


def projected_failure_outcome(outcome: dict[str, Any]) -> dict[str, Any]:
    projected = copy.deepcopy(outcome)
    content = projected.get("content")
    if isinstance(content, str) and len(content) > 16 * 1024:
        projected["content"] = (
            f"[verifier 输出已确定性压缩，canonical 记录保留 {len(content)} 字符]\n"
            f"{content[: 3 * 2_048]}\n…\n{content[-2_048:]}"
        )
    return projected


def failure_fact_projection_audit(
    request: dict[str, Any] | None,
    facts: dict[str, Any] | None,
    required: bool,
) -> dict[str, Any]:
    if facts is None:
        checks = {
            "rejection_identity": False,
            "failure_outcome": False,
            "failure_workspace": False,
        }
        return {
            "required": required,
            "observed_exact": False,
            "valid": not required,
            "audit_sha256": canonical_hash(checks),
            "source_content_sha256": None,
            "projected_outcome_sha256": None,
        }
    messages = request.get("messages") if isinstance(request, dict) else None
    contents = [
        message.get("content")
        for message in messages or []
        if isinstance(message, dict) and isinstance(message.get("content"), str)
    ]
    visible = "\n".join(contents)
    projected_outcome = projected_failure_outcome(facts["outcome"])
    encoded = {
        "rejection_identity": json.dumps(
            facts["rejection"],
            ensure_ascii=False,
            separators=(",", ":"),
        ),
        "failure_outcome": json.dumps(
            projected_outcome,
            ensure_ascii=False,
            separators=(",", ":"),
        ),
        "failure_workspace": json.dumps(
            facts["workspace_state"],
            ensure_ascii=False,
            separators=(",", ":"),
        ),
    }
    checks = {name: value in visible for name, value in encoded.items()}
    observed = all(checks.values())
    content = facts["outcome"].get("content")
    return {
        "required": required,
        "observed_exact": observed,
        "valid": observed if required else True,
        "audit_sha256": canonical_hash(checks),
        "source_content_sha256": (
            canonical_hash(content) if isinstance(content, str) else None
        ),
        "projected_outcome_sha256": canonical_hash(projected_outcome),
    }


def baseline_compaction_retry_lifecycle_valid(
    events: list[dict[str, Any]],
    run: dict[str, Any],
    committed: dict[str, Any],
) -> bool:
    kinds = [event_kind(stored) for stored in events]
    prepared = [
        stored["event"]
        for stored in events
        if event_kind(stored) == "context_compaction_prepared"
    ]
    inflight = [
        stored["event"]
        for stored in events
        if event_kind(stored) == "context_compaction_in_flight"
    ]
    failures = [
        stored["event"]
        for stored in events
        if event_kind(stored) == "context_compaction_attempt_failed"
    ]
    if len(prepared) != 1 or len(inflight) != len(failures) + 1:
        return False
    compaction_id = committed.get("compaction_id")
    initial = prepared[0]
    retry_prepared = [
        failure.get("retry", {}).get("prepared")
        for failure in failures
    ]
    if not all(
        isinstance(item, dict)
        and isinstance(item.get("request"), dict)
        and failure.get("retry", {}).get("decision") == "retry"
        for failure, item in zip(failures, retry_prepared)
    ):
        return False
    attempts = [
        {
            "attempt_id": initial.get("attempt_id"),
            "request": initial.get("request"),
        },
        *retry_prepared,
    ]
    expected_order = ["run_created", "context_compaction_prepared"]
    for _ in failures:
        expected_order.extend(
            ["context_compaction_in_flight", "context_compaction_attempt_failed"]
        )
    expected_order.extend(
        [
            "context_compaction_in_flight",
            "context_compaction_committed",
            "terminal",
        ]
    )
    return all(
        (
            kinds == expected_order,
            isinstance(initial.get("request"), dict),
            initial.get("compaction_id") == compaction_id,
            initial.get("trigger") == committed.get("trigger"),
            all(item.get("compaction_id") == compaction_id for item in inflight),
            all(item.get("compaction_id") == compaction_id for item in failures),
            [item.get("attempt_id") for item in inflight]
            == [item.get("attempt_id") for item in attempts],
            [item.get("attempt_id") for item in failures]
            == [item.get("attempt_id") for item in attempts[:-1]],
            [
                (item.get("request") or {}).get("attempt")
                for item in attempts
            ]
            == list(range(len(attempts))),
            run.get("runtime_model_requests") == len(attempts),
            run.get("runtime_retries") == len(failures),
        )
    )


def compact_audit(
    variant: str,
    events: list[dict[str, Any]],
    run: dict[str, Any],
) -> dict[str, Any]:
    kinds = [event_kind(stored) for stored in events]
    committed = [
        stored["event"]
        for stored in events
        if event_kind(stored) == "context_compaction_committed"
    ]
    created = run_created_request(events)
    source_transcript = created.get("transcript")
    transcript_valid = isinstance(source_transcript, dict) and isinstance(
        source_transcript.get("entries"), list
    )
    base = {
        "exercised": len(committed) == 1,
        "transcript_unchanged": transcript_valid,
        "event_order": [
            kind
            for kind in kinds
            if kind not in {"content_delta", "reasoning_delta"}
        ],
        "before_tokens": committed[0].get("before_tokens") if committed else None,
        "after_tokens": committed[0].get("after_tokens") if committed else None,
        "projection_sha256": (
            canonical_hash(committed[0].get("projection")) if committed else None
        ),
    }
    if len(committed) != 1:
        return {**base, "valid": False, "local": variant == "candidate"}
    event = committed[0]
    before = event.get("before_tokens")
    after = event.get("after_tokens")
    projection = event.get("projection")
    token_valid = (
        isinstance(before, int)
        and isinstance(after, int)
        and before > after > 0
    )
    if variant == "baseline":
        retry_lifecycle = baseline_compaction_retry_lifecycle_valid(
            events, run, event
        )
        valid = all(
            (
                transcript_valid,
                token_valid,
                retry_lifecycle,
                event.get("output") is not None,
                isinstance(projection, dict),
                projection.get("summary_prompt") is not None
                if isinstance(projection, dict)
                else False,
            )
        )
        return {
            **base,
            "valid": valid,
            "local": False,
            "attempts": run.get("runtime_model_requests"),
            "retries": run.get("runtime_retries"),
        }

    old_kinds = {
        "context_compaction_prepared",
        "context_compaction_in_flight",
        "context_compaction_attempt_failed",
    }
    indices = projection.get("selected_entry_indices") if isinstance(projection, dict) else None
    messages = projection.get("messages") if isinstance(projection, dict) else None
    source_entries = source_transcript.get("entries")
    selection_valid = (
        isinstance(indices, list)
        and isinstance(messages, list)
        and isinstance(source_entries, list)
        and len(indices) == len(messages)
        and all(isinstance(index, int) for index in indices)
        and all(left < right for left, right in zip(indices, indices[1:]))
        and all(0 <= index < len(source_entries) for index in indices)
        and projection.get("source_entry_count") == len(source_entries)
        and HELPER.valid_sha256(projection.get("source_projection_sha256"))
    )
    user_mapping_valid = selection_valid and all(
        source_entries[index].get("kind") != "user"
        or message
        == {"role": "user", "content": source_entries[index].get("content")}
        for index, message in zip(indices or [], messages or [])
    )
    accounting = accounting_metrics(run, events)
    valid = all(
        (
            transcript_valid,
            token_valid,
            not old_kinds.intersection(kinds),
            kinds
            == ["run_created", "context_compaction_committed", "terminal"],
            run.get("runtime_model_requests") == 0,
            accounting["api_requests"] == 0,
            accounting["usage"]["total_tokens"] == 0,
            accounting["cost_usd"] == 0,
            selection_valid,
            user_mapping_valid,
            isinstance(event.get("tools"), list),
        )
    )
    return {**base, "valid": valid, "local": True}


def marker_visibility(request: dict[str, Any] | None, scenario: str) -> dict[str, Any]:
    if request is None:
        return {"count": 0, "all": False, "audit_sha256": None}
    visible = json.dumps(
        {
            "system_prompt": request.get("system_prompt"),
            "messages": request.get("messages"),
        },
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
    )
    checks = [value in visible for value in marker_values(scenario)]
    return {
        "count": sum(checks),
        "all": all(checks),
        "audit_sha256": canonical_hash(checks),
    }


def source_phase_task(scenario: str, phase: int) -> dict[str, Any]:
    objective = phase_objective(scenario, phase)
    if scenario == "task-b" and phase == PHASES:
        return verifier_task(scenario, objective)
    return host_task(objective)


def create_source(
    server: Any,
    workspace: Path,
    scenario: str,
    model: str,
    variant: str,
) -> tuple[list[dict[str, Any]], str, dict[str, Any] | None, dict[str, Any]]:
    roots: list[dict[str, Any]] = []
    run, events = submit_run(
        server,
        start_command(workspace, model, source_phase_task(scenario, 1)),
    )
    roots.append({"run": run, "events": events})
    for phase in range(2, PHASES + 1):
        protect_source = scenario == "task-b" and phase == PHASES
        if protect_source:
            set_source_failure_workspace_read_only(workspace, True)
        try:
            run, events = submit_run(
                server,
                {
                    "kind": "continue",
                    "run_id": run["run_id"],
                    "task": source_phase_task(scenario, phase),
                    "expected_workspace": run["workspace"],
                },
            )
        finally:
            if protect_source:
                set_source_failure_workspace_read_only(workspace, False)
        roots.append({"run": run, "events": events})
    expected_states = ["completed"] * PHASES
    if SCENARIOS[scenario]["source_failure"]:
        expected_states[-1] = "blocked"
    states = [terminal_name(root["events"]) for root in roots]
    schema_valid = all(
        event_schema_valid(root["events"], variant) for root in roots
    )
    failure_valid = (
        validate_expected_source_failure(events, run, scenario)
        if SCENARIOS[scenario]["source_failure"]
        else "completion_rejected" not in [event_kind(item) for item in events]
    )
    source_request = next(
        (
            stored["event"].get("request")
            for stored in events
            if event_kind(stored) == "run_created"
        ),
        None,
    )
    transcript = (
        source_request.get("transcript")
        if isinstance(source_request, dict)
        else None
    )
    transcript_valid = isinstance(transcript, dict) and isinstance(
        transcript.get("entries"), list
    )
    last_counts = Counter(event_kind(item) for item in events)
    commits = [
        item["event"]
        for item in events
        if event_kind(item) == "host_verification_committed"
    ]
    audit = {
        "valid": all(
            (
                states == expected_states,
                schema_valid,
                failure_valid,
                transcript_valid,
            )
        ),
        "terminal_states_valid": states == expected_states,
        "event_schema_valid": schema_valid,
        "expected_failure_valid": failure_valid,
        "transcript_valid": transcript_valid,
        "last_failure_kind": terminal_failure_kind(events),
        "last_event_kind_counts": {
            kind: last_counts[kind]
            for kind in (
                "completion_proposed",
                "host_verification_prepared",
                "host_verification_started",
                "host_verification_committed",
                "completion_rejected",
            )
        },
        "receipt_present": any(
            commit.get("receipt") is not None for commit in commits
        ),
    }
    return (
        roots,
        run["run_id"],
        transcript if transcript_valid else None,
        audit,
    )


def arm_order(variant: str, scenario: str, repetition: int) -> tuple[str, str]:
    parity = (
        (0 if variant == "baseline" else 1)
        + (0 if scenario == "task-a" else 1)
        + repetition
    )
    return ("off", "on") if parity % 2 == 0 else ("on", "off")


def run_arm(
    server: Any,
    source_run_id: str,
    scenario: str,
    mode: str,
    variant: str,
    workspace: Path,
    source_facts: dict[str, Any] | None,
) -> dict[str, Any]:
    arm_started = time.monotonic()
    compact_run = None
    compact_events = None
    continuation_source = source_run_id
    if mode == "on":
        compact_run, compact_events = submit_run(
            server,
            {
                "kind": "compact",
                "run_id": source_run_id,
                "expected_workspace": str(workspace.resolve()),
            },
        )
        continuation_source = compact_run["run_id"]
        if terminal_name(compact_events) != "context_compaction_completed":
            raise EvaluationError("compaction_terminal_invalid")

    final_run, final_events = submit_run(
        server,
        {
            "kind": "continue",
            "run_id": continuation_source,
            "task": final_task(scenario),
            "expected_workspace": str(workspace.resolve()),
        },
    )
    duration = time.monotonic() - arm_started
    external = run_external_verifier(scenario, workspace)
    receipt = validate_receipt(final_events, final_run, scenario)
    final_created = run_created_request(final_events)
    final_transcript = final_created.get("transcript")
    source_transcript = (
        run_created_request(compact_events).get("transcript")
        if compact_events is not None
        else final_transcript
    )
    source_continuation_valid = (
        compact_run is None
        or compact_run.get("continued_from_run_id") == source_run_id
    )
    final_lineage_valid = all(
        (
            source_continuation_valid,
            final_run.get("continued_from_run_id") == continuation_source,
            isinstance(final_transcript, dict),
            isinstance(source_transcript, dict),
            final_transcript == source_transcript,
        )
    )
    compaction_commits = (
        [
            stored["event"]
            for stored in compact_events
            if event_kind(stored) == "context_compaction_committed"
        ]
        if compact_events is not None
        else []
    )
    committed_projection = (
        compaction_commits[0].get("projection")
        if len(compaction_commits) == 1
        else None
    )
    expected_projection = committed_projection if mode == "on" else None
    source_projection = (
        run_created_request(compact_events).get("context_projection")
        if compact_events is not None
        else None
    )
    projection_lineage_valid = all(
        (
            source_projection is None,
            final_created.get("context_projection") == expected_projection,
            (mode == "off") or committed_projection is not None,
        )
    )
    compact = (
        compact_audit(
            variant,
            compact_events,
            compact_run,
        )
        if compact_run is not None and compact_events is not None
        else {
            "valid": True,
            "exercised": False,
            "local": None,
            "transcript_unchanged": True,
            "before_tokens": None,
            "after_tokens": None,
            "projection_sha256": None,
        }
    )
    compact["transcript_unchanged"] = final_lineage_valid
    compact["projection_inherited"] = projection_lineage_valid
    compact["valid"] = (
        compact["valid"] and final_lineage_valid and projection_lineage_valid
    )
    final_kinds = [event_kind(stored) for stored in final_events]
    unexpected_compaction = any(
        kind.startswith("context_compaction_") for kind in final_kinds
    )
    visibility = marker_visibility(first_model_request(final_events), scenario)
    fact_projection = failure_fact_projection_audit(
        first_model_request(final_events),
        source_facts,
        required=variant == "candidate" and scenario == "task-b",
    )
    final_metrics, final_accounting_audit = accounting_assessment(
        final_run, final_events
    )
    compact_metrics, compact_accounting_audit = (
        accounting_assessment(compact_run, compact_events)
        if compact_run is not None and compact_events is not None
        else (
            sum_metrics([]),
            {
                "valid": True,
                "api_started": 0,
                "runtime_model_requests": 0,
                "runtime_retries": 0,
                "transport_retries": 0,
                "axes": {},
                "physical_minus_logical": {
                    field: 0 for field in USAGE_FIELDS
                },
            },
        )
    )
    terminal = terminal_name(final_events)
    verified_success = all(
        (
            terminal == "completed",
            external["passed"],
            receipt["valid"],
            visibility["all"],
            final_lineage_valid,
            projection_lineage_valid,
            fact_projection["valid"],
        )
    )
    false_success = terminal == "completed" and not all(
        (external["passed"], receipt["valid"])
    )
    measurement_valid = all(
        (
            event_schema_valid(final_events, variant),
            final_metrics["valid"],
            compact_metrics["valid"],
            external["valid"],
            compact["valid"],
            final_lineage_valid,
            projection_lineage_valid,
            fact_projection["valid"],
            not unexpected_compaction,
            compact["exercised"] == (mode == "on"),
        )
    )
    _, terminal_event = terminal_state(final_events)
    terminal_failure = terminal_event.get("failure")
    measurement_axes = {
        "event_schema": event_schema_valid(final_events, variant),
        "final_accounting": final_metrics["valid"],
        "compact_accounting": compact_metrics["valid"],
        "external_verifier": external["valid"],
        "compaction_contract": compact["valid"],
        "lineage": final_lineage_valid,
        "projection_lineage": projection_lineage_valid,
        "failure_fact_projection": fact_projection["valid"],
        "no_automatic_compaction": not unexpected_compaction,
        "mode_exercised": compact["exercised"] == (mode == "on"),
    }
    return {
        "mode": mode,
        "final_run_id_sha256": canonical_hash(final_run["run_id"]),
        "terminal_state": terminal,
        "verified_success": verified_success,
        "false_success": false_success,
        "measurement_valid": measurement_valid,
        "event_schema_version": EXPECTED_EVENT_SCHEMA[variant],
        "final_event_prefix_sha256": canonical_hash(final_events),
        "final_projection_sha256": (
            canonical_hash(first_model_request(final_events))
            if first_model_request(final_events) is not None
            else None
        ),
        "marker_visibility": visibility,
        "receipt_valid": receipt["valid"],
        "receipt_axes": {
            key: receipt[key]
            for key in ("gate_valid", "gate_exercised", "contract_valid")
        },
        "receipt_sha256": receipt["receipt_sha256"],
        "external_verifier": external,
        "lineage_valid": final_lineage_valid,
        "projection_lineage_valid": projection_lineage_valid,
        "failure_fact_projection": fact_projection,
        "source_transcript_sha256": (
            canonical_hash(source_transcript)
            if isinstance(source_transcript, dict)
            else None
        ),
        "source_transcript_entries": (
            len(source_transcript.get("entries", []))
            if isinstance(source_transcript, dict)
            else None
        ),
        "measurement_axes": measurement_axes,
        "terminal_error_code": (
            terminal_failure.get("kind")
            if isinstance(terminal_failure, dict)
            else None
        ),
        "final_event_kind_counts": dict(
            sorted(
                Counter(
                    event_kind(stored)
                    for stored in final_events
                    if event_kind(stored)
                    not in {"content_delta", "reasoning_delta"}
                ).items()
            )
        ),
        "compaction": compact,
        "compact_metrics": compact_metrics,
        "compact_accounting_audit": compact_accounting_audit,
        "final_metrics": final_metrics,
        "final_accounting_audit": final_accounting_audit,
        "arm_duration_seconds": round(duration, 3),
        "changed_files": HELPER.changed_files(
            initialize_snapshot_for(scenario), HELPER.snapshot_workspace(workspace)
        ),
    }


def initialize_snapshot_for(scenario: str) -> dict[str, Any]:
    return HELPER.snapshot_workspace(Path(SCENARIOS[scenario]["fixture"]))


def source_stage_audit(scenario: str, workspace: Path) -> dict[str, Any]:
    if scenario != "task-b":
        return {
            "required": False,
            "valid": True,
            "placeholder_exact": None,
            "external_valid": None,
            "public_behavior_failing": None,
            "changed_file_boundary": None,
        }
    try:
        module = ast.parse(
            (workspace / str(SCENARIOS[scenario]["target"])).read_text(
                encoding="utf-8"
            )
        )
    except (OSError, UnicodeError, SyntaxError):
        module = None
    values: list[str] = []
    for node in module.body if module is not None else []:
        if (
            isinstance(node, (ast.Assign, ast.AnnAssign))
            and isinstance(node.value, ast.Constant)
            and isinstance(node.value.value, str)
        ):
            targets = node.targets if isinstance(node, ast.Assign) else [node.target]
            if any(
                isinstance(target, ast.Name) and target.id == "CONTEXT_PROOF"
                for target in targets
            ):
                values.append(node.value.value)
    external = run_external_verifier(scenario, workspace)
    checks = external.get("checks", {})
    placeholder_exact = values == ["source-stage-incomplete"]
    public_behavior_failing = checks.get("public_tests") is False
    changed_file_boundary = checks.get("immutable_files") is True
    valid = all(
        (
            placeholder_exact,
            external["valid"],
            not external["passed"],
            public_behavior_failing,
            checks.get("hidden_cases") is False,
            changed_file_boundary,
        )
    )
    return {
        "required": True,
        "valid": valid,
        "placeholder_exact": placeholder_exact,
        "external_valid": external["valid"],
        "public_behavior_failing": public_behavior_failing,
        "changed_file_boundary": changed_file_boundary,
    }


def run_pair(
    variant: str,
    binary: Path,
    scenario: str,
    repetition: int,
    model: str,
    key: str,
) -> dict[str, Any]:
    pair_started = time.monotonic()
    with tempfile.TemporaryDirectory(
        prefix=f"codewhale-m5b-{variant}-{scenario}-"
    ) as raw:
        root = Path(raw)
        workspace = root / "workspace"
        state_root = root / "state"
        backup = root / "source-workspace"
        initial = initialize_workspace(scenario, workspace)
        environment = HELPER.child_environment(key, state_root)
        source_started = time.monotonic()
        server = HELPER.StdioRunApi(binary, 5, workspace, environment)
        try:
            source_roots, source_run_id, source_transcript, source_creation_audit = create_source(
                server, workspace, scenario, model, variant
            )
        finally:
            server.close()
        source_duration = time.monotonic() - source_started
        source_workspace = HELPER.snapshot_workspace(workspace)
        source_changed = HELPER.changed_files(initial, source_workspace)
        expected_source_changed = (
            [str(SCENARIOS[scenario]["target"])]
            if SCENARIOS[scenario]["source_failure"]
            else []
        )
        mutation_exercised = any(
            stored["event"].get("outcome", {}).get("side_effect") == "applied"
            and stored["event"].get("workspace_state") is not None
            for stored in source_roots[PHASES - 2]["events"]
            if event_kind(stored) == "tool_outcome_committed"
        )
        stage_audit = source_stage_audit(scenario, workspace)
        source_facts, source_failure_fact_audit = (
            source_failure_fact_assessment(source_roots[-1]["events"])
        )
        source_assessments = [
            accounting_assessment(root_data["run"], root_data["events"])
            for root_data in source_roots
        ]
        source_metrics = [metrics for metrics, _ in source_assessments]
        source_aggregate = sum_metrics(source_metrics)
        source_accounting_audits = [
            {"phase": phase, **audit}
            for phase, (_, audit) in enumerate(source_assessments, start=1)
        ]
        source_compaction_free = not any(
            event_kind(stored).startswith("context_compaction_")
            for root_data in source_roots
            for stored in root_data["events"]
        )
        expected_failure_exercised = (
            validate_expected_source_failure(
                source_roots[-1]["events"],
                source_roots[-1]["run"],
                scenario,
            )
            if SCENARIOS[scenario]["source_failure"]
            else False
        )
        source_contract_axes = {
            "creation_contract": source_creation_audit["valid"],
            "workspace_change_boundary": (
                source_changed == expected_source_changed
            ),
            "mutation_contract": (
                not SCENARIOS[scenario]["source_failure"]
                or mutation_exercised
            ),
            "stage_contract": stage_audit["valid"],
            "failure_fact_contract": (
                source_failure_fact_audit["valid"]
                and SCENARIOS[scenario]["source_failure"]
                == (source_facts is not None)
            ),
            "source_compaction_free": source_compaction_free,
        }
        source_contract = {
            "run_id_sha256": canonical_hash(source_run_id),
            "terminal_states": [
                terminal_name(root_data["events"]) for root_data in source_roots
            ],
            "event_prefix_sha256": canonical_hash(
                [canonical_hash(root_data["events"]) for root_data in source_roots]
            ),
            "last_phase_input_transcript_sha256": (
                canonical_hash(source_transcript)
                if source_transcript is not None
                else None
            ),
            "last_phase_input_transcript_entries": (
                len(source_transcript["entries"])
                if source_transcript is not None
                else None
            ),
            "initial_workspace_sha256": canonical_hash(initial),
            "source_workspace_sha256": canonical_hash(source_workspace),
            "changed_files": source_changed,
            "mutation_exercised": mutation_exercised,
            "stage_audit": stage_audit,
            "failure_fact_audit": source_failure_fact_audit,
            "creation_audit": source_creation_audit,
            "contract_axes": source_contract_axes,
            "expected_failure_exercised": expected_failure_exercised,
            "host_failure_cycles": sum(
                event_kind(stored) == "host_verification_committed"
                for stored in source_roots[-1]["events"]
            ),
            "automatic_compaction_absent": source_compaction_free,
            "duration_seconds": round(source_duration, 3),
            "metrics": source_aggregate,
            "accounting_audits": source_accounting_audits,
            "invalid_accounting_phases": [
                audit["phase"]
                for audit in source_accounting_audits
                if not audit["valid"]
            ],
        }
        if not all(source_contract_axes.values()):
            return {
                "pair_id": f"{variant}:{scenario}:{repetition}",
                "variant": variant,
                "scenario": scenario,
                "repetition": repetition,
                "measurement_valid": False,
                "resample_eligible": False,
                "invalid_stage": "source_contract",
                "source": source_contract,
                "arm_order": [],
                "arms": [],
                "pair_duration_seconds": round(time.monotonic() - pair_started, 3),
            }
        if not source_aggregate["valid"] or not source_compaction_free:
            invalid_source_audits = [
                audit for audit in source_accounting_audits if not audit["valid"]
            ]
            return {
                "pair_id": f"{variant}:{scenario}:{repetition}",
                "variant": variant,
                "scenario": scenario,
                "repetition": repetition,
                "measurement_valid": False,
                "resample_eligible": (
                    source_compaction_free
                    and bool(invalid_source_audits)
                    and all(
                        accounting_gap_resample_eligible(audit)
                        for audit in invalid_source_audits
                    )
                ),
                "invalid_stage": "source",
                "source": source_contract,
                "arm_order": [],
                "arms": [],
                "pair_duration_seconds": round(time.monotonic() - pair_started, 3),
            }
        if source_transcript is None:
            raise EvaluationError("validated_source_transcript_missing")
        shutil.copytree(workspace, backup, symlinks=True)
        arms = []
        for mode in arm_order(variant, scenario, repetition):
            restore_workspace(backup, workspace)
            server = HELPER.StdioRunApi(binary, 5, workspace, environment)
            try:
                arm = run_arm(
                    server,
                    source_run_id,
                    scenario,
                    mode,
                    variant,
                    workspace,
                    source_facts,
                )
            finally:
                server.close()
            arm["chain_metrics"] = sum_metrics(
                [source_aggregate, arm["compact_metrics"], arm["final_metrics"]]
            )
            arm["chain_duration_seconds"] = round(
                source_duration + arm["arm_duration_seconds"], 3
            )
            arm["measurement_axes"]["source_accounting"] = True
            arm["measurement_axes"]["source_compaction_free"] = True
            arms.append(arm)
            if not arm["measurement_valid"]:
                source_contract["shared_arm_transcript"] = None
                source_contract["terminal_transcript_sha256"] = arm[
                    "source_transcript_sha256"
                ]
                source_contract["terminal_transcript_entries"] = arm[
                    "source_transcript_entries"
                ]
                pair_contract_valid = all(
                    (
                        source_aggregate["valid"],
                        source_compaction_free,
                    )
                )
                return {
                    "pair_id": f"{variant}:{scenario}:{repetition}",
                    "variant": variant,
                    "scenario": scenario,
                    "repetition": repetition,
                    "measurement_valid": False,
                    "resample_eligible": pair_resample_eligible(
                        arms, pair_contract_valid
                    ),
                    "invalid_stage": "treatment_arm",
                    "source": source_contract,
                    "arm_order": [item["mode"] for item in arms],
                    "arms": arms,
                    "pair_duration_seconds": round(
                        time.monotonic() - pair_started, 3
                    ),
                }
        source_digests = {arm["source_transcript_sha256"] for arm in arms}
        shared_source_transcript = len(source_digests) == 1 and None not in source_digests
        for arm in arms:
            arm["measurement_axes"]["shared_source_transcript"] = (
                shared_source_transcript
            )
            arm["measurement_axes"]["source_accounting"] = source_aggregate["valid"]
            arm["measurement_axes"]["source_compaction_free"] = source_compaction_free
            arm["measurement_valid"] = (
                arm["measurement_valid"]
                and shared_source_transcript
                and source_aggregate["valid"]
                and source_compaction_free
            )
        source_contract["shared_arm_transcript"] = shared_source_transcript
        source_contract["terminal_transcript_sha256"] = arms[0][
            "source_transcript_sha256"
        ]
        source_contract["terminal_transcript_entries"] = arms[0][
            "source_transcript_entries"
        ]
        measurement_valid = all(arm["measurement_valid"] for arm in arms)
        pair_contract_valid = all(
            (
                shared_source_transcript,
                source_aggregate["valid"],
                source_compaction_free,
            )
        )
        return {
            "pair_id": f"{variant}:{scenario}:{repetition}",
            "variant": variant,
            "scenario": scenario,
            "repetition": repetition,
            "measurement_valid": measurement_valid,
            "resample_eligible": (
                not measurement_valid
                and pair_resample_eligible(arms, pair_contract_valid)
            ),
            "invalid_stage": (
                None if measurement_valid else "treatment_arm"
            ),
            "source": source_contract,
            "arm_order": [arm["mode"] for arm in arms],
            "arms": arms,
            "pair_duration_seconds": round(time.monotonic() - pair_started, 3),
        }


def metric_summary(values: list[float | int]) -> dict[str, float | int]:
    return {
        "total": round(sum(values), 9),
        "mean": round(statistics.fmean(values), 9),
        "median": round(statistics.median(values), 9),
    }


def arm_metric(arm: dict[str, Any], metric: str) -> float:
    if metric == "api_requests":
        return float(arm["chain_metrics"]["api_requests"])
    if metric == "tokens":
        return float(arm["chain_metrics"]["usage"]["total_tokens"])
    if metric == "input_tokens":
        return float(arm["chain_metrics"]["usage"]["input_tokens"])
    if metric == "cost_usd":
        return float(arm["chain_metrics"]["cost_usd"])
    if metric == "duration_seconds":
        return float(arm["chain_duration_seconds"])
    raise EvaluationError(f"unknown_pair_metric:{metric}")


def paired_comparison(
    pairs: list[dict[str, Any]],
    variant: str,
    scenario: str | None,
) -> dict[str, Any]:
    selected = [
        pair
        for pair in pairs
        if pair["variant"] == variant
        and (scenario is None or pair["scenario"] == scenario)
    ]
    metrics: dict[str, Any] = {}
    for metric in (
        "api_requests",
        "tokens",
        "input_tokens",
        "cost_usd",
        "duration_seconds",
    ):
        deltas = []
        percentages = []
        for pair in selected:
            by_mode = {arm["mode"]: arm for arm in pair["arms"]}
            off = arm_metric(by_mode["off"], metric)
            on = arm_metric(by_mode["on"], metric)
            delta = on - off
            deltas.append(delta)
            if off != 0:
                percentages.append(delta / off * 100)
        metrics[metric] = {
            "delta": metric_summary(deltas),
            "delta_percent": (
                metric_summary(percentages)
                if len(percentages) == len(deltas)
                else None
            ),
            "paired_lower_count": sum(delta < 0 for delta in deltas),
            "paired_equal_count": sum(delta == 0 for delta in deltas),
            "paired_higher_count": sum(delta > 0 for delta in deltas),
        }
    return {
        "variant": variant,
        "scenario": scenario or "all",
        "pairs": len(selected),
        "verified_success_delta": sum(
            next(arm for arm in pair["arms"] if arm["mode"] == "on")[
                "verified_success"
            ]
            - next(arm for arm in pair["arms"] if arm["mode"] == "off")[
                "verified_success"
            ]
            for pair in selected
        ),
        "false_success_delta": sum(
            next(arm for arm in pair["arms"] if arm["mode"] == "on")[
                "false_success"
            ]
            - next(arm for arm in pair["arms"] if arm["mode"] == "off")[
                "false_success"
            ]
            for pair in selected
        ),
        "on_minus_off": metrics,
    }


def aggregate(
    pairs: list[dict[str, Any]],
    runs_per_cell: int,
    invalid_attempts: list[dict[str, Any]] | None = None,
) -> dict[str, Any]:
    invalid_attempts = invalid_attempts or []
    arms = []
    for pair in pairs:
        for arm in pair["arms"]:
            arms.append(
                {
                    "variant": pair["variant"],
                    "scenario": pair["scenario"],
                    "repetition": pair["repetition"],
                    **arm,
                }
            )
    invalid_outcome_arms = [
        {
            "variant": attempt["variant"],
            "scenario": attempt["scenario"],
            "repetition": attempt["repetition"],
            "measurement_attempt": attempt.get("measurement_attempt"),
            **arm,
        }
        for attempt in invalid_attempts
        for arm in attempt["arms"]
    ]
    outcome_arms = [*arms, *invalid_outcome_arms]
    cells: dict[tuple[str, str, str], list[dict[str, Any]]] = defaultdict(list)
    for arm in arms:
        cells[(arm["variant"], arm["scenario"], arm["mode"])].append(arm)
    outcome_cells: dict[
        tuple[str, str, str], list[dict[str, Any]]
    ] = defaultdict(list)
    for arm in outcome_arms:
        outcome_cells[(arm["variant"], arm["scenario"], arm["mode"])].append(
            arm
        )
    summaries = []
    for (variant, scenario, mode), rows in sorted(cells.items()):
        observed = outcome_cells[(variant, scenario, mode)]
        summaries.append(
            {
                "variant": variant,
                "scenario": scenario,
                "mode": mode,
                "runs": len(rows),
                "verified_success": sum(row["verified_success"] for row in rows),
                "false_success": sum(row["false_success"] for row in rows),
                "observed_outcomes": len(observed),
                "observed_verified_success": sum(
                    row["verified_success"] for row in observed
                ),
                "observed_false_success": sum(
                    row["false_success"] for row in observed
                ),
                "observed_success_rate": round(
                    sum(row["verified_success"] for row in observed)
                    / len(observed),
                    9,
                ),
                "api_requests": metric_summary(
                    [row["chain_metrics"]["api_requests"] for row in rows]
                ),
                "tokens": metric_summary(
                    [row["chain_metrics"]["usage"]["total_tokens"] for row in rows]
                ),
                "input_tokens": metric_summary(
                    [row["chain_metrics"]["usage"]["input_tokens"] for row in rows]
                ),
                "output_tokens": metric_summary(
                    [row["chain_metrics"]["usage"]["output_tokens"] for row in rows]
                ),
                "cost_usd": metric_summary(
                    [row["chain_metrics"]["cost_usd"] for row in rows]
                ),
                "duration_seconds": metric_summary(
                    [row["chain_duration_seconds"] for row in rows]
                ),
                "measurement_valid": all(row["measurement_valid"] for row in rows),
            }
        )
    expected_cells = {
        (variant, scenario, mode)
        for variant in ("baseline", "candidate")
        for scenario in SCENARIOS
        for mode in ("off", "on")
    }
    exact_pairs = {
        (pair["variant"], pair["scenario"], pair["repetition"]) for pair in pairs
    } == {
        (variant, scenario, repetition)
        for variant in ("baseline", "candidate")
        for scenario in SCENARIOS
        for repetition in range(1, runs_per_cell + 1)
    }
    exact_cells = set(cells) == expected_cells and all(
        len(rows) == runs_per_cell
        and {row["repetition"] for row in rows}
        == set(range(1, runs_per_cell + 1))
        for rows in cells.values()
    )
    product_eligible = all(
        (
            exact_pairs,
            exact_cells,
            all(arm["measurement_valid"] for arm in arms),
            all(
                arm["compaction"]["exercised"] == (arm["mode"] == "on")
                for arm in arms
            ),
        )
    )
    summaries_by_key = {
        (summary["variant"], summary["scenario"], summary["mode"]): summary
        for summary in summaries
    }
    comparisons = []
    for variant in ("baseline", "candidate"):
        comparisons.extend(
            paired_comparison(pairs, variant, scenario)
            for scenario in (*SCENARIOS.keys(), None)
        )
    candidate_rows = [
        arm for arm in arms if arm["variant"] == "candidate" and arm["mode"] == "on"
    ]
    candidate_outcomes = [
        arm
        for arm in outcome_arms
        if arm["variant"] == "candidate" and arm["mode"] == "on"
    ]
    candidate_all_success = (
        len(candidate_outcomes) >= len(SCENARIOS) * runs_per_cell
        and all(row["verified_success"] for row in candidate_outcomes)
        and not any(row["false_success"] for row in candidate_outcomes)
    )
    reliability_non_regression = product_eligible and candidate_all_success and all(
        summaries_by_key[("candidate", scenario, "on")][
            "observed_success_rate"
        ]
        >= summaries_by_key[("candidate", scenario, "off")][
            "observed_success_rate"
        ]
        and summaries_by_key[("candidate", scenario, "on")][
            "observed_success_rate"
        ]
        >= summaries_by_key[("baseline", scenario, "on")][
            "observed_success_rate"
        ]
        for scenario in SCENARIOS
    )
    comparisons_by_key = {
        (comparison["variant"], comparison["scenario"]): comparison
        for comparison in comparisons
    }
    candidate_overall = comparisons_by_key[("candidate", "all")]

    def paired_percent(
        comparison: dict[str, Any], metric: str, statistic: str
    ) -> float | None:
        summary = comparison["on_minus_off"][metric]["delta_percent"]
        return (
            float(summary[statistic])
            if isinstance(summary, dict) and summary.get(statistic) is not None
            else None
        )

    efficiency_benefit = (
        reliability_non_regression
        and all(
            candidate_overall["on_minus_off"][metric]["paired_lower_count"]
            >= len(candidate_rows) - 1
            for metric in ("tokens", "cost_usd")
        )
        and all(
            value is not None and value <= -5.0
            for metric in ("tokens", "cost_usd")
            for value in (
                paired_percent(candidate_overall, metric, "mean"),
                paired_percent(candidate_overall, metric, "median"),
            )
        )
        and all(
            value is not None and value <= -5.0
            for scenario in SCENARIOS
            for metric in ("tokens", "cost_usd")
            for value in (
                paired_percent(
                    comparisons_by_key[("candidate", scenario)], metric, "median"
                ),
            )
        )
        and candidate_overall["on_minus_off"]["api_requests"][
            "paired_higher_count"
        ]
        == 0
        and all(
            value is not None and value <= 20.0
            for value in (
                paired_percent(candidate_overall, "duration_seconds", "median"),
                *(
                    paired_percent(
                        comparisons_by_key[("candidate", scenario)],
                        "duration_seconds",
                        "median",
                    )
                    for scenario in SCENARIOS
                ),
            )
        )
    )
    reliability_benefit = (
        reliability_non_regression
        and all(
            summaries_by_key[("candidate", scenario, "on")][
                "observed_success_rate"
            ]
            > summaries_by_key[("candidate", scenario, "off")][
                "observed_success_rate"
            ]
            and summaries_by_key[("candidate", scenario, "on")][
                "observed_success_rate"
            ]
            > summaries_by_key[("baseline", scenario, "on")][
                "observed_success_rate"
            ]
            for scenario in SCENARIOS
        )
        and all(
            float(summaries_by_key[("candidate", scenario, "on")][metric]["median"])
            <= float(summaries_by_key[("baseline", scenario, "on")][metric]["median"])
            * 1.2
            for scenario in SCENARIOS
            for metric in ("cost_usd", "duration_seconds")
        )
    )
    keep_threshold = (
        product_eligible
        and reliability_non_regression
        and (efficiency_benefit or reliability_benefit)
    )
    if not product_eligible:
        candidate_decision = "measurement_invalid"
    elif not candidate_all_success:
        candidate_decision = "delete"
    elif keep_threshold:
        candidate_decision = "keep"
    else:
        candidate_decision = "shrink"
    physical_metrics = []
    for pair in pairs:
        physical_metrics.append(pair["source"]["metrics"])
        physical_metrics.extend(
            metric
            for arm in pair["arms"]
            for metric in (arm["compact_metrics"], arm["final_metrics"])
        )
    return {
        "product_metric_eligible": product_eligible,
        "candidate_reliability_non_regression_met": reliability_non_regression,
        "candidate_efficiency_benefit_met": efficiency_benefit,
        "candidate_reliability_benefit_met": reliability_benefit,
        "candidate_keep_threshold_met": keep_threshold,
        "candidate_decision": candidate_decision,
        "exact_pairs": exact_pairs,
        "exact_cells": exact_cells,
        "invalid_treatment_outcomes": len(invalid_outcome_arms),
        "cells": summaries,
        "paired_comparisons": comparisons,
        "physical_suite_metrics": sum_metrics(physical_metrics),
    }


def schedule(runs_per_cell: int) -> list[tuple[str, str, int]]:
    result = []
    for scenario_index, scenario in enumerate(SCENARIOS):
        for repetition in range(1, runs_per_cell + 1):
            variants = (
                ("baseline", "candidate")
                if (scenario_index + repetition) % 2
                else ("candidate", "baseline")
            )
            result.extend((variant, scenario, repetition) for variant in variants)
    return result


def binary_identity(path: Path, expected_revision: str) -> dict[str, Any]:
    identity = HELPER.executable_identity(path)
    resolved_expected = HELPER.resolve_repo_revision(expected_revision)
    if identity["source_revision"] != resolved_expected:
        raise EvaluationError("binary_revision_mismatch")
    return {
        "sha256": identity["sha256"],
        "version_sha256": canonical_hash(identity["version"]),
        "source_revision": identity["source_revision"],
    }


def complexity_record(baseline_revision: str, candidate_revision: str) -> dict[str, Any]:
    completed = subprocess.run(
        [
            "git",
            "diff",
            "--numstat",
            baseline_revision,
            candidate_revision,
            "--",
            ":(glob)crates/*/src/**/*.rs",
        ],
        cwd=ROOT,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        timeout=30,
        check=False,
    )
    if completed.returncode != 0:
        raise EvaluationError("complexity_diff_failed")
    added = deleted = 0
    for line in completed.stdout.splitlines():
        parts = line.split("\t", 2)
        if len(parts) != 3 or not parts[0].isdigit() or not parts[1].isdigit():
            continue
        added += int(parts[0])
        deleted += int(parts[1])
    return {
        "production_insertions": added,
        "production_deletions": deleted,
        "production_net": added - deleted,
        "new_model_visible_tools": 0,
        "new_dependencies": 0,
        "new_product_configuration_switches": 0,
        "new_persistent_truths": 0,
        "new_runtime_event_variants": 0,
        "removed_runtime_event_variants": 3,
        "new_model_call_sites": 0,
        "agent_runtime_owners": 1,
        "run_store_owners": 1,
        "compaction_owners": 1,
    }


def fixture_manifest() -> dict[str, Any]:
    return {
        scenario: {
            "workspace_sha256": canonical_hash(
                HELPER.snapshot_workspace(Path(definition["fixture"]))
            ),
            "marker_set_sha256": canonical_hash(marker_values(scenario)),
            "expected_proof_sha256": canonical_hash(expected_proof(scenario)),
            "task_sha256": canonical_hash(final_task(scenario)),
        }
        for scenario, definition in SCENARIOS.items()
    }


def assert_redacted(payload: dict[str, Any], key: str) -> None:
    encoded = HELPER.canonical_bytes(payload)
    forbidden = [key, *sum((marker_values(scenario) for scenario in SCENARIOS), [])]
    forbidden.extend(expected_proof(scenario) for scenario in SCENARIOS)
    if any(value and value.encode("utf-8") in encoded for value in forbidden):
        raise EvaluationError("result_contains_forbidden_secret_or_marker")
    temporary_prefixes = (b"/private/tmp/codewhale-m5b-", b"/tmp/codewhale-m5b-")
    if any(prefix in encoded for prefix in temporary_prefixes):
        raise EvaluationError("result_contains_temporary_path")


def dry_plan(args: argparse.Namespace) -> dict[str, Any]:
    identities = {
        "baseline": binary_identity(args.baseline_bin, args.baseline_revision),
        "candidate": binary_identity(args.candidate_bin, args.candidate_revision),
    }
    if identities["baseline"]["sha256"] == identities["candidate"]["sha256"]:
        raise EvaluationError("baseline_candidate_binary_equal")
    return {
        "schema": RESULT_SCHEMA,
        "mode": "dry_run",
        "model": args.model,
        "runs_per_cell": args.runs_per_cell,
        "max_pair_attempts": MAX_PAIR_ATTEMPTS,
        "planned_pairs": len(SCENARIOS) * 2 * args.runs_per_cell,
        "planned_arms": len(SCENARIOS) * 4 * args.runs_per_cell,
        "maximum_pair_attempts": (
            len(SCENARIOS) * 2 * args.runs_per_cell * MAX_PAIR_ATTEMPTS
        ),
        "schedule": [
            {"variant": variant, "scenario": scenario, "repetition": repetition}
            for variant, scenario, repetition in schedule(args.runs_per_cell)
        ],
        "identities": identities,
        "fixtures": fixture_manifest(),
        "verifier_sha256": file_hash(VERIFIER),
        "complexity": complexity_record(
            identities["baseline"]["source_revision"],
            identities["candidate"]["source_revision"],
        ),
    }


class HarnessSelfTests(unittest.TestCase):
    def test_terminal_accounting_projects_seal_and_runtime_retry(self) -> None:
        projected = {
            "sealed": False,
            "runtime_retries": 2,
            "usage": {"input_tokens": 10},
            "cost_nanousd": 7,
        }
        run = {
            "accounting": projected,
            "runtime_model_requests": 3,
            "runtime_retries": 1,
            "tool_calls": 2,
        }
        terminal = {
            "accounting": {
                **projected,
                "sealed": True,
                "runtime_retries": 3,
            },
            "runtime_model_requests": 3,
            "runtime_retries": 1,
            "tool_calls": 2,
        }
        self.assertTrue(terminal_accounting_projection_valid(terminal, run))
        terminal["accounting"]["runtime_retries"] = 2
        self.assertFalse(terminal_accounting_projection_valid(terminal, run))

    def test_physical_usage_contains_retry_usage_without_hiding_regression(self) -> None:
        fields = {field: 0 for field in USAGE_FIELDS}
        logical = {**fields, "input_tokens": 100, "output_tokens": 20}
        physical = {**logical, "input_tokens": 150, "output_tokens": 30}
        run = {
            "accounting": {"usage": physical, "transport_retries": 0},
            "usage": logical,
            "runtime_retries": 1,
        }
        self.assertTrue(run_usage_projection_valid(run))
        run["runtime_retries"] = 0
        self.assertFalse(run_usage_projection_valid(run))
        run["runtime_retries"] = 1
        run["accounting"]["usage"]["input_tokens"] = 99
        self.assertFalse(run_usage_projection_valid(run))

    def test_resampling_only_accepts_retry_backed_observability_gaps(self) -> None:
        axes = {
            "helper_valid": False,
            "terminal_projection_valid": True,
            "usage_projection_valid": True,
            "model_request_limit_valid": True,
            "hard_request_limit_valid": True,
            "api_request_limit_valid": True,
            "accounting_invariants_valid": True,
            "normalized_observability_valid": True,
            "complete": False,
            "usage_complete": True,
            "usage_missing_absent": True,
            "usage_incomplete_absent": True,
            "billing_known": False,
            "priced": True,
            "budget_not_exhausted": True,
            "no_in_flight": True,
            "no_records_after_seal": True,
        }
        audit = {
            "valid": False,
            "runtime_retries": 1,
            "transport_retries": 0,
            "axes": axes,
        }
        self.assertTrue(accounting_gap_resample_eligible(audit))
        audit["runtime_retries"] = 0
        self.assertFalse(accounting_gap_resample_eligible(audit))
        audit["runtime_retries"] = 1
        axes["terminal_projection_valid"] = False
        self.assertFalse(accounting_gap_resample_eligible(audit))

        axes["terminal_projection_valid"] = True
        arm = {
            "measurement_valid": False,
            "verified_success": True,
            "false_success": False,
            "terminal_state": "completed",
            "measurement_axes": {
                "final_accounting": False,
                "compact_accounting": True,
                "lineage": True,
            },
            "final_accounting_audit": audit,
            "compact_accounting_audit": {"valid": True},
        }
        self.assertTrue(arm_resample_eligible(arm))
        self.assertTrue(pair_resample_eligible([arm], True))
        self.assertFalse(pair_resample_eligible([arm], False))
        failed_outcome = {
            **arm,
            "measurement_valid": True,
            "verified_success": False,
            "terminal_state": "blocked",
        }
        self.assertTrue(
            pair_resample_eligible([arm, failed_outcome], True)
        )
        arm["verified_success"] = False
        arm["terminal_state"] = "blocked"
        self.assertTrue(arm_resample_eligible(arm))
        arm["verified_success"] = True
        arm["terminal_state"] = "completed"
        arm["measurement_axes"]["lineage"] = False
        self.assertFalse(arm_resample_eligible(arm))

    def test_accounting_invariants_reject_counter_or_flag_corruption(self) -> None:
        accounting = {
            "root": {
                "started": 2,
                "completed": 2,
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
            "sealed": False,
            "complete": False,
            "usage_complete": True,
            "usage_missing": False,
            "usage_incomplete": False,
            "billing_unknown": True,
            "unpriced": False,
            "usage_responses": 1,
            "usage_missing_responses": 0,
            "incomplete_responses": 0,
            "billing_unknown_attempts": 1,
            "unpriced_usage_responses": 0,
            "records_after_seal": 0,
            "cost_nanousd": 10,
            "cost_nanocny": 70,
        }
        self.assertTrue(accounting_invariants_valid(accounting))
        for field, bad_value in (
            ("sealed_denied", 1),
            ("transport_retries", 1),
            ("billing_unknown_attempts", 0),
            ("unpriced_usage_responses", 1),
            ("usage_missing_responses", 3),
            ("incomplete_responses", 3),
        ):
            corrupted = {**accounting, field: bad_value}
            self.assertFalse(accounting_invariants_valid(corrupted))
        corrupted = copy.deepcopy(accounting)
        corrupted["root"]["completed"] = 1
        self.assertFalse(accounting_invariants_valid(corrupted))

    def test_full_accounting_assessment_accepts_only_retry_observability_gap(
        self,
    ) -> None:
        usage = {field: 0 for field in USAGE_FIELDS}
        usage["input_tokens"] = 100
        usage["output_tokens"] = 20

        def assess(
            *,
            usage_complete: bool,
            usage_incomplete: bool,
            billing_unknown: bool,
        ) -> dict[str, Any]:
            accounting = {
                "hard_request_limit": MAX_API_REQUESTS,
                "root": {
                    "started": 2,
                    "completed": 2,
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
                "sealed": False,
                "complete": False,
                "usage_complete": usage_complete,
                "usage_missing": False,
                "usage_incomplete": usage_incomplete,
                "billing_unknown": billing_unknown,
                "unpriced": False,
                "usage_responses": 1,
                "usage_missing_responses": 0,
                "incomplete_responses": int(usage_incomplete),
                "billing_unknown_attempts": int(billing_unknown),
                "unpriced_usage_responses": 0,
                "records_after_seal": 0,
                "usage": usage,
                "surface_usage": [],
                "cost_nanousd": 10,
                "cost_nanocny": 70,
            }
            run = {
                "accounting": accounting,
                "usage": usage,
                "runtime_model_requests": 2,
                "runtime_retries": 1,
                "tool_calls": 0,
            }
            terminal_accounting = {
                **accounting,
                "sealed": True,
                "runtime_retries": 1,
            }
            events = [
                {
                    "event": {
                        "kind": "terminal",
                        "outcome": {
                            "terminal": {"state": "completed"},
                            "accounting": terminal_accounting,
                            "runtime_model_requests": 2,
                            "runtime_retries": 1,
                            "tool_calls": 0,
                        },
                    }
                }
            ]
            _, audit = accounting_assessment(run, events)
            return audit

        unknown = assess(
            usage_complete=True,
            usage_incomplete=False,
            billing_unknown=True,
        )
        self.assertTrue(accounting_gap_resample_eligible(unknown))
        incomplete = assess(
            usage_complete=False,
            usage_incomplete=True,
            billing_unknown=False,
        )
        self.assertTrue(accounting_gap_resample_eligible(incomplete))

    def test_source_failure_fact_assessment_never_drops_partial_protocol(
        self,
    ) -> None:
        commit = {
            "event": {
                "kind": "host_verification_committed",
                "outcome": {},
                "receipt": None,
            }
        }
        rejection = {
            "event": {
                "kind": "completion_rejected",
                "rejection": {"candidate_id": "candidate-1"},
            }
        }
        for events in ([commit], [rejection], [commit, rejection]):
            facts, audit = source_failure_fact_assessment(events)
            self.assertIsNone(facts)
            self.assertFalse(audit["valid"])
        facts, audit = source_failure_fact_assessment([])
        self.assertIsNone(facts)
        self.assertTrue(audit["valid"])

    def test_invalid_attempt_costs_remain_known_minima(self) -> None:
        def metrics(valid: bool, requests: int, tokens: int) -> dict[str, Any]:
            usage = {field: 0 for field in USAGE_FIELDS}
            usage["input_tokens"] = tokens
            usage["total_tokens"] = tokens
            return {
                "valid": valid,
                "api_requests": requests,
                "runtime_model_requests": requests,
                "runtime_retries": 0,
                "tool_calls": 0,
                "usage": usage,
                "cost_usd": tokens / 1_000_000,
                "cost_cny": tokens / 100_000,
            }

        invalid = {
            "execution_metrics": metrics(False, 2, 200),
            "pair_duration_seconds": 2.5,
        }
        accepted = {
            "execution_metrics": metrics(True, 1, 100),
            "pair_duration_seconds": 1.5,
        }
        summary = measurement_execution_summary([accepted], [invalid])
        self.assertEqual(summary["total_pair_attempts"], 2)
        self.assertEqual(summary["invalid_pair_attempts"], 1)
        self.assertEqual(summary["pair_attempt_duration_seconds"], 4.0)
        self.assertEqual(summary["invalid_attempt_duration_seconds"], 2.5)
        self.assertFalse(summary["accounting_complete"])
        self.assertEqual(summary["cost_semantics"], "known_minimum")
        self.assertEqual(
            summary["known_minimum_metrics"]["usage"]["total_tokens"], 300
        )

    def test_source_retry_aggregation_preserves_physical_minima(self) -> None:
        def metrics(valid: bool, requests: int) -> dict[str, Any]:
            usage = {field: 0 for field in USAGE_FIELDS}
            usage["input_tokens"] = requests * 10
            usage["total_tokens"] = requests * 10
            return {
                "valid": valid,
                "api_requests": requests,
                "runtime_model_requests": requests,
                "runtime_retries": requests - 1,
                "tool_calls": 0,
                "usage": usage,
                "cost_usd": requests / 1_000,
                "cost_cny": requests / 100,
            }

        complete = [metrics(True, 2), *[metrics(True, 1) for _ in range(8)]]
        complete_aggregate = sum_metrics(complete)
        self.assertTrue(complete_aggregate["valid"])
        self.assertEqual(complete_aggregate["api_requests"], 10)
        self.assertEqual(complete_aggregate["usage"]["total_tokens"], 100)

        complete[0]["valid"] = False
        unknown_aggregate = sum_metrics(complete)
        self.assertFalse(unknown_aggregate["valid"])
        self.assertEqual(unknown_aggregate["api_requests"], 10)
        self.assertEqual(unknown_aggregate["usage"]["total_tokens"], 100)

    def test_baseline_compaction_retry_lifecycle_is_exact(self) -> None:
        def stored(kind: str, **payload: Any) -> dict[str, Any]:
            return {"event": {"kind": kind, **payload}}

        committed = {
            "kind": "context_compaction_committed",
            "compaction_id": "compaction-1",
            "trigger": "manual",
        }
        events = [
            stored("run_created"),
            stored(
                "context_compaction_prepared",
                compaction_id="compaction-1",
                attempt_id="attempt-0",
                trigger="manual",
                request={"attempt": 0},
            ),
            stored(
                "context_compaction_in_flight",
                compaction_id="compaction-1",
                attempt_id="attempt-0",
            ),
            stored(
                "context_compaction_attempt_failed",
                compaction_id="compaction-1",
                attempt_id="attempt-0",
                retry={
                    "decision": "retry",
                    "prepared": {
                        "attempt_id": "attempt-1",
                        "request": {"attempt": 1},
                    },
                },
            ),
            stored(
                "context_compaction_in_flight",
                compaction_id="compaction-1",
                attempt_id="attempt-1",
            ),
            {"event": committed},
            stored("terminal"),
        ]
        run = {"runtime_model_requests": 2, "runtime_retries": 1}
        self.assertTrue(
            baseline_compaction_retry_lifecycle_valid(events, run, committed)
        )
        events[4]["event"]["attempt_id"] = "wrong-attempt"
        self.assertFalse(
            baseline_compaction_retry_lifecycle_valid(events, run, committed)
        )

    def test_schedule_has_twelve_pairs_and_twenty_four_arms(self) -> None:
        planned = schedule(3)
        self.assertEqual(len(planned), 12)
        counts = Counter((variant, scenario) for variant, scenario, _ in planned)
        self.assertEqual(set(counts.values()), {3})
        orders = [
            arm_order(variant, scenario, repetition)
            for variant, scenario, repetition in planned
        ]
        self.assertEqual(sum(order[0] == "on" for order in orders), 6)

    def test_markers_are_high_entropy_and_stable(self) -> None:
        for scenario in SCENARIOS:
            values = marker_values(scenario)
            self.assertEqual(len(values), PHASES)
            self.assertEqual(len(set(values)), PHASES)
            self.assertTrue(all(len(value) == 10 for value in values))
            self.assertEqual(expected_proof(scenario), "".join(values))

    def test_frozen_fixtures_fail_then_known_fixes_pass(self) -> None:
        replacements = {
            "task-a": (
                "ranges.py",
                "        start = previous\n        previous = current",
                "        start = current\n        previous = current",
            ),
            "task-b": (
                "settings.py",
                "    for key, value in override.items():\n        merged[key] = value",
                "    for key, value in override.items():\n"
                "        if isinstance(value, dict) and isinstance(merged.get(key), dict):\n"
                "            merged[key] = merge_settings(merged[key], value)\n"
                "        else:\n"
                "            merged[key] = value",
            ),
        }
        for scenario, (target, old, new) in replacements.items():
            with self.subTest(scenario=scenario), tempfile.TemporaryDirectory(
                prefix="codewhale-m5b-self-test-"
            ) as raw:
                workspace = Path(raw) / "workspace"
                initialize_workspace(scenario, workspace)
                self.assertFalse(run_external_verifier(scenario, workspace)["passed"])
                path = workspace / target
                body = path.read_text(encoding="utf-8").replace(old, new)
                path.write_text(
                    f'{body}\n\nCONTEXT_PROOF = "{expected_proof(scenario)}"\n',
                    encoding="utf-8",
                )
                result = run_external_verifier(scenario, workspace)
                self.assertTrue(result["valid"])
                self.assertTrue(result["passed"])

    def test_task_b_source_stage_is_broken_but_well_formed(self) -> None:
        with tempfile.TemporaryDirectory(prefix="codewhale-m5b-self-test-") as raw:
            workspace = Path(raw) / "workspace"
            initialize_workspace("task-b", workspace)
            target = workspace / "settings.py"
            target.write_text(
                target.read_text(encoding="utf-8")
                + '\n\nCONTEXT_PROOF = "source-stage-incomplete"\n',
                encoding="utf-8",
            )
            audit = source_stage_audit("task-b", workspace)
            self.assertTrue(audit["valid"])
            self.assertTrue(audit["placeholder_exact"])
            self.assertTrue(audit["public_behavior_failing"])
            self.assertTrue(audit["changed_file_boundary"])

    def test_pair_delta_is_computed_before_summary(self) -> None:
        def arm(mode: str, value: int) -> dict[str, Any]:
            return {
                "mode": mode,
                "verified_success": True,
                "false_success": False,
                "chain_metrics": {
                    "api_requests": value,
                    "usage": {
                        "total_tokens": value,
                        "input_tokens": value,
                    },
                    "cost_usd": float(value),
                },
                "chain_duration_seconds": float(value),
            }

        pairs = [
            {
                "variant": "candidate",
                "scenario": "task-a",
                "repetition": repetition,
                "arms": [arm("off", off), arm("on", on)],
            }
            for repetition, (off, on) in enumerate(
                ((1, 2), (100, 3), (101, 110)), start=1
            )
        ]
        comparison = paired_comparison(pairs, "candidate", "task-a")
        tokens = comparison["on_minus_off"]["tokens"]
        self.assertEqual(tokens["delta"]["median"], 1.0)
        self.assertEqual(tokens["paired_lower_count"], 1)
        self.assertEqual(tokens["paired_higher_count"], 2)

    def test_cross_revision_gain_cannot_fake_compaction_keep(self) -> None:
        def metrics(value: int) -> dict[str, Any]:
            usage = {field: 0 for field in USAGE_FIELDS}
            usage.update(
                {
                    "input_tokens": value * 10,
                    "output_tokens": value,
                    "total_tokens": value * 11,
                }
            )
            return {
                "valid": True,
                "api_requests": value,
                "runtime_model_requests": value,
                "runtime_retries": 0,
                "tool_calls": 0,
                "usage": usage,
                "cost_usd": value / 1_000,
                "cost_cny": value / 500,
            }

        def arm(mode: str, value: int, success: bool) -> dict[str, Any]:
            return {
                "mode": mode,
                "verified_success": success,
                "false_success": False,
                "measurement_valid": True,
                "compaction": {"exercised": mode == "on"},
                "chain_metrics": metrics(value),
                "chain_duration_seconds": float(value),
                "compact_metrics": metrics(0),
                "final_metrics": metrics(value),
            }

        pairs = []
        for variant in ("baseline", "candidate"):
            for scenario in SCENARIOS:
                for repetition in range(1, 4):
                    baseline_on_success = not (
                        variant == "baseline" and repetition == 1
                    )
                    pairs.append(
                        {
                            "variant": variant,
                            "scenario": scenario,
                            "repetition": repetition,
                            "source": {"metrics": metrics(1)},
                            "arms": [
                                arm("off", 10, True),
                                arm(
                                    "on",
                                    11,
                                    baseline_on_success
                                    if variant == "baseline"
                                    else True,
                                ),
                            ],
                        }
                    )
        result = aggregate(pairs, 3)
        self.assertTrue(result["candidate_reliability_non_regression_met"])
        self.assertFalse(result["candidate_efficiency_benefit_met"])
        self.assertFalse(result["candidate_reliability_benefit_met"])
        self.assertEqual(result["candidate_decision"], "shrink")

        invalid_attempt = {
            "variant": "candidate",
            "scenario": "task-a",
            "repetition": 1,
            "measurement_attempt": 1,
            "arms": [
                {
                    "mode": "on",
                    "verified_success": False,
                    "false_success": False,
                }
            ],
        }
        with_invalid_outcome = aggregate(pairs, 3, [invalid_attempt])
        self.assertTrue(with_invalid_outcome["product_metric_eligible"])
        self.assertEqual(
            with_invalid_outcome["invalid_treatment_outcomes"], 1
        )
        self.assertFalse(
            with_invalid_outcome[
                "candidate_reliability_non_regression_met"
            ]
        )
        self.assertEqual(with_invalid_outcome["candidate_decision"], "delete")
        candidate_task_a_on = next(
            cell
            for cell in with_invalid_outcome["cells"]
            if (
                cell["variant"],
                cell["scenario"],
                cell["mode"],
            )
            == ("candidate", "task-a", "on")
        )
        self.assertEqual(candidate_task_a_on["observed_outcomes"], 4)
        self.assertEqual(
            candidate_task_a_on["observed_success_rate"], 0.75
        )

    def test_verifier_specs_are_exact_and_hidden_from_model_message(self) -> None:
        for scenario in SCENARIOS:
            spec = verifier_spec(scenario)
            self.assertEqual(spec["parameters"]["profile"], "exact")
            self.assertEqual(spec["parameters"]["commands"][0]["cwd"], "")
            self.assertEqual(spec["plan"]["steps"][0]["env"], {})
            self.assertEqual(
                spec["plan"]["steps"][0]["timeout_ms"], HOST_VERIFIER_TIMEOUT_MS
            )
            message = task_model_message(final_task(scenario))
            self.assertNotIn(expected_proof(scenario), message)
            self.assertNotIn(str(VERIFIER), message)


def run_self_tests() -> int:
    suite = unittest.defaultTestLoader.loadTestsFromTestCase(HarnessSelfTests)
    return 0 if unittest.TextTestRunner(verbosity=2).run(suite).wasSuccessful() else 1


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__)
    mode = result.add_mutually_exclusive_group()
    mode.add_argument("--self-test", action="store_true")
    mode.add_argument("--dry-run", action="store_true")
    result.add_argument("--acknowledge-cost", action="store_true")
    result.add_argument("--key-file", type=Path)
    result.add_argument("--baseline-bin", type=Path)
    result.add_argument("--baseline-revision")
    result.add_argument("--candidate-bin", type=Path)
    result.add_argument("--candidate-revision")
    result.add_argument("--model", default=MODEL)
    result.add_argument("--runs-per-cell", type=int, default=RUNS_PER_CELL)
    result.add_argument(
        "--output",
        type=Path,
        default=ROOT / "eval/results/m5-context-broker.json",
    )
    return result


def require_live_arguments(args: argparse.Namespace) -> None:
    required = (
        args.baseline_bin,
        args.baseline_revision,
        args.candidate_bin,
        args.candidate_revision,
    )
    if any(value is None for value in required):
        raise EvaluationError("binary_and_revision_arguments_required")
    if args.runs_per_cell < 1:
        raise EvaluationError("runs_per_cell_must_be_positive")
    if not args.dry_run and (not args.acknowledge_cost or args.key_file is None):
        raise EvaluationError("live_run_requires_cost_acknowledgement_and_key")


def main() -> int:
    args = parser().parse_args()
    if args.self_test:
        return run_self_tests()
    require_live_arguments(args)
    if args.dry_run:
        print(
            json.dumps(
                dry_plan(args),
                ensure_ascii=False,
                sort_keys=True,
                separators=(",", ":"),
            )
        )
        return 0

    key = HELPER.load_key(args.key_file)
    plan = dry_plan(args)
    binaries = {
        "baseline": args.baseline_bin.resolve(),
        "candidate": args.candidate_bin.resolve(),
    }
    pairs: list[dict[str, Any]] = []
    invalid_attempts: list[dict[str, Any]] = []
    planned_schedule = schedule(args.runs_per_cell)
    suite_started = time.monotonic()
    for index, (variant, scenario, repetition) in enumerate(
        planned_schedule, start=1
    ):
        accepted = False
        for measurement_attempt in range(1, MAX_PAIR_ATTEMPTS + 1):
            try:
                pair = run_pair(
                    variant,
                    binaries[variant],
                    scenario,
                    repetition,
                    args.model,
                    key,
                )
            except EvaluationError as error:
                execution = measurement_execution_summary(
                    pairs, invalid_attempts
                )
                execution["unaccounted_pair_attempts"] = 1
                execution["accounting_complete"] = False
                execution["cost_semantics"] = "known_minimum"
                progress = {
                    "schema": RESULT_SCHEMA,
                    "mode": "aborted_harness_error",
                    "completed_pairs": len(pairs),
                    "planned_pairs": len(planned_schedule),
                    "completed_pair_attempts": (
                        len(pairs) + len(invalid_attempts)
                    ),
                    "max_pair_attempts": MAX_PAIR_ATTEMPTS,
                    "suite_duration_seconds": round(
                        time.monotonic() - suite_started, 3
                    ),
                    "pairs": pairs,
                    "invalid_attempts": invalid_attempts,
                    "measurement_execution": execution,
                    "abort": {
                        "reason": "harness_error",
                        "reason_sha256": canonical_hash(str(error)),
                        "pair_id": f"{variant}:{scenario}:{repetition}",
                        "measurement_attempt": measurement_attempt,
                    },
                }
                assert_redacted(progress, key)
                HELPER.atomic_write_json(
                    args.output.with_suffix(args.output.suffix + ".partial"),
                    progress,
                )
                raise
            pair["schedule_index"] = index
            pair["measurement_attempt"] = measurement_attempt
            pair["execution_metrics"] = pair_attempt_metrics(pair)
            if pair["measurement_valid"]:
                pairs.append(pair)
                accepted = True
            else:
                invalid_attempts.append(pair)
            progress = {
                "schema": RESULT_SCHEMA,
                "mode": "partial",
                "completed_pairs": len(pairs),
                "planned_pairs": len(planned_schedule),
                "completed_pair_attempts": len(pairs) + len(invalid_attempts),
                "max_pair_attempts": MAX_PAIR_ATTEMPTS,
                "suite_duration_seconds": round(
                    time.monotonic() - suite_started, 3
                ),
                "pairs": pairs,
                "invalid_attempts": invalid_attempts,
                "measurement_execution": measurement_execution_summary(
                    pairs, invalid_attempts
                ),
            }
            assert_redacted(progress, key)
            HELPER.atomic_write_json(
                args.output.with_suffix(args.output.suffix + ".partial"),
                progress,
            )
            if accepted:
                break
            if not pair["resample_eligible"]:
                progress["mode"] = "aborted_measurement_invalid"
                progress["abort"] = {
                    "reason": "not_resample_eligible",
                    "pair_id": pair["pair_id"],
                    "measurement_attempt": measurement_attempt,
                    "invalid_stage": pair["invalid_stage"],
                }
                assert_redacted(progress, key)
                HELPER.atomic_write_json(
                    args.output.with_suffix(args.output.suffix + ".partial"),
                    progress,
                )
                raise EvaluationError(
                    "pair_measurement_invalid_not_resample_eligible:"
                    f"{pair['pair_id']}:attempt={measurement_attempt}:"
                    f"stage={pair['invalid_stage']}"
                )
        if not accepted:
            progress["mode"] = "aborted_measurement_invalid"
            progress["abort"] = {
                "reason": "attempts_exhausted",
                "pair_id": f"{variant}:{scenario}:{repetition}",
                "measurement_attempt": MAX_PAIR_ATTEMPTS,
                "invalid_stage": pair["invalid_stage"],
            }
            assert_redacted(progress, key)
            HELPER.atomic_write_json(
                args.output.with_suffix(args.output.suffix + ".partial"),
                progress,
            )
            raise EvaluationError(
                "pair_measurement_attempts_exhausted:"
                f"{variant}:{scenario}:{repetition}:"
                f"max_attempts={MAX_PAIR_ATTEMPTS}"
            )

    result = {
        "schema": RESULT_SCHEMA,
        "mode": "formal_ab",
        "created_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "model": args.model,
        "runs_per_cell": args.runs_per_cell,
        "plan_sha256": canonical_hash(plan),
        "identities": plan["identities"],
        "fixtures": plan["fixtures"],
        "verifier_sha256": plan["verifier_sha256"],
        "complexity": plan["complexity"],
        "pairs": pairs,
        "invalid_attempts": invalid_attempts,
        "measurement_execution": measurement_execution_summary(
            pairs, invalid_attempts
        ),
        "aggregate": aggregate(pairs, args.runs_per_cell, invalid_attempts),
        "suite_duration_seconds": round(time.monotonic() - suite_started, 3),
        "interpretation": {
            "treatment": "manual_compaction_on_off_within_each_revision",
            "cross_revision_scope": "entire_m5b_vertical_slice",
            "statistical_claim": "repeated_direction_only_not_statistical_significance",
            "resampling": (
                "whole_pair_only_for_retry_backed_accounting_observability_gap;"
                f"maximum_{MAX_PAIR_ATTEMPTS}_attempts;"
                "never_task_outcome_or_product_contract"
            ),
        },
    }
    assert_redacted(result, key)
    HELPER.atomic_write_json(args.output, result)
    args.output.with_suffix(args.output.suffix + ".partial").unlink(missing_ok=True)
    print(
        json.dumps(
            {
                "output": str(args.output),
                "product_metric_eligible": result["aggregate"][
                    "product_metric_eligible"
                ],
                "candidate_decision": result["aggregate"]["candidate_decision"],
                "candidate_keep_threshold_met": result["aggregate"][
                    "candidate_keep_threshold_met"
                ],
                "pairs": len(pairs),
                "arms": sum(len(pair["arms"]) for pair in pairs),
                "invalid_pair_attempts": len(invalid_attempts),
                "total_pair_attempts": len(pairs) + len(invalid_attempts),
            },
            ensure_ascii=False,
            sort_keys=True,
            separators=(",", ":"),
        )
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except EvaluationError as error:
        print(f"evaluation_error:{error}", file=sys.stderr)
        raise SystemExit(2)
