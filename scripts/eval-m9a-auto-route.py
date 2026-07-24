#!/usr/bin/env python3
"""M9-A same-binary Host Auto release-admission evaluator.

The evaluator compares fixed Pro, fixed Flash, and the Host-owned Auto policy
through the canonical app-server Run API.  It persists terminal RunStore facts
before reopen, verifier, route, accounting, or product-metric derivation.
"""

from __future__ import annotations

import argparse
from collections import Counter
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import signal
import stat
import statistics
import subprocess
import sys
import tempfile
import time
from typing import Any, BinaryIO
import uuid


ROOT = Path(__file__).resolve().parents[1]
MANIFEST_PATH = ROOT / "eval/manifests/m9-a-host-auto-release-admission-v1.json"
FIXTURE_ROOT = ROOT / "eval/fixtures/m7-readonly-fanout"
RUN_API = 11
EVENT_API = 17
STATE_SCHEMA = 23
EXEC_STREAM = 3
MANIFEST_SCHEMA = "codewhale.eval.m9-a-host-auto-release-admission.v1"
JOURNAL_SCHEMA = "codewhale.eval.m9-a-host-auto-journal.v1"
ADMISSION_SCHEMA = "codewhale.eval.m9-a-host-auto-live-admission.v1"
ZERO_HASH = "sha256:" + ("0" * 64)
VARIANTS = ("fixed_pro", "fixed_flash", "host_auto")
RUNS_PER_CELL = 3
MAX_API_REQUESTS = 16
MAX_TURNS = 16
MAX_TOOL_CALLS = 32
MAX_OUTPUT_TOKENS = 4096
WALL_TIME_SECONDS = 360
MODEL_IDLE_MS = 120_000
HARNESS_GRACE_SECONDS = 30
PER_ARM_COST_CEILING_USD = 0.04
MAX_FRAME = 16 * 1024 * 1024
ROOT_TOOLS = [
    "apply_patch",
    "edit_file",
    "file_search",
    "grep_files",
    "list_dir",
    "read_file",
    "run_verifiers",
    "agent",
]
MODELS = {
    "fixed_pro": "deepseek-v4-pro",
    "fixed_flash": "deepseek-v4-flash",
    "host_auto": None,
}
EXPECTED_ROUTES = {
    "fixed_pro": {
        "root_model": "deepseek-v4-pro",
        "child_model": "deepseek-v4-pro",
        "root_mode": "explicit",
        "root_reason": "explicit_model",
        "child_reason": "explicit_model_inherited",
        "policy": "deepseek_explicit_v1",
    },
    "fixed_flash": {
        "root_model": "deepseek-v4-flash",
        "child_model": "deepseek-v4-flash",
        "root_mode": "explicit",
        "root_reason": "explicit_model",
        "child_reason": "explicit_model_inherited",
        "policy": "deepseek_explicit_v1",
    },
    "host_auto": {
        "root_model": "deepseek-v4-pro",
        "child_model": "deepseek-v4-flash",
        "root_mode": "auto",
        "root_reason": "auto_root_responsible",
        "child_reason": "auto_read_only_investigation",
        "policy": "deepseek_host_auto_v1",
    },
}


def load_legacy_helpers() -> Any:
    path = ROOT / "scripts/eval-m7g-readonly-fanout.py"
    spec = importlib.util.spec_from_file_location("codewhale_m7g_helpers", path)
    if spec is None or spec.loader is None:
        raise RuntimeError("legacy_helper_import_failed")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    module.RUN_API = RUN_API
    module.EVENT_API = EVENT_API
    module.STATE_SCHEMA = STATE_SCHEMA
    module.MAX_API_REQUESTS = MAX_API_REQUESTS
    module.MAX_TURNS = MAX_TURNS
    module.MAX_TOOL_CALLS = MAX_TOOL_CALLS
    module.MAX_OUTPUT_TOKENS = MAX_OUTPUT_TOKENS
    module.WALL_TIME_SECONDS = WALL_TIME_SECONDS
    module.MODEL_IDLE_MS = MODEL_IDLE_MS
    module.MAX_FRAME = MAX_FRAME
    return module


M7G = load_legacy_helpers()
TASKS = M7G.TASKS


EvaluationError = M7G.EvaluationError


def require(
    condition: bool, code: str, details: dict[str, Any] | None = None
) -> None:
    if not condition:
        raise EvaluationError(code, details)


def canonical_bytes(value: Any) -> bytes:
    return json.dumps(
        value,
        allow_nan=False,
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")


def sha256_bytes(value: bytes) -> str:
    return "sha256:" + hashlib.sha256(value).hexdigest()


def file_hash(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return "sha256:" + digest.hexdigest()


def canonical_hash(value: Any) -> str:
    return sha256_bytes(canonical_bytes(value))


def git_output(*arguments: str, cwd: Path = ROOT) -> str:
    result = M7G.run_command(["git", *arguments], cwd=cwd)
    require(result.returncode == 0, "git_failed")
    try:
        return result.stdout.decode("utf-8").strip()
    except UnicodeDecodeError as error:
        raise EvaluationError("git_output_invalid") from error


def fsync_directory(directory: Path) -> None:
    descriptor = os.open(directory, os.O_RDONLY)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def write_all(descriptor: int, value: bytes) -> None:
    view = memoryview(value)
    while view:
        written = os.write(descriptor, view)
        require(written > 0, "journal_write_failed")
        view = view[written:]


class Journal:
    def __init__(self, path: Path, stream: BinaryIO) -> None:
        self.path = path
        self.stream = stream
        self.sequence = 0
        self.previous_record_sha256 = ZERO_HASH

    @classmethod
    def claim(
        cls, path: Path, *, enforce_results_scope: bool = True
    ) -> "Journal":
        require(path.is_absolute(), "output_must_be_absolute")
        if enforce_results_scope:
            require(
                path.parent.resolve() == (ROOT / "eval/results").resolve(),
                "output_scope_invalid",
            )
        try:
            descriptor = os.open(
                path,
                os.O_WRONLY
                | os.O_CREAT
                | os.O_EXCL
                | os.O_APPEND
                | getattr(os, "O_NOFOLLOW", 0),
                0o600,
            )
        except OSError as error:
            raise EvaluationError("output_claim_failed") from error
        os.fchmod(descriptor, 0o600)
        fsync_directory(path.parent)
        return cls(path, os.fdopen(descriptor, "wb", buffering=0))

    def __enter__(self) -> "Journal":
        return self

    def __exit__(self, *_: object) -> None:
        self.stream.close()

    def emit(
        self, payload: dict[str, Any], *, fault: str | None = None
    ) -> str:
        core = {
            "schema": JOURNAL_SCHEMA,
            "sequence": self.sequence + 1,
            "previous_record_sha256": self.previous_record_sha256,
            "payload": payload,
        }
        record_sha256 = canonical_hash(core)
        encoded = canonical_bytes(
            {**core, "record_sha256": record_sha256}
        ) + b"\n"
        descriptor = self.stream.fileno()
        if fault == "mid_write_kill":
            write_all(descriptor, encoded[: max(1, len(encoded) // 2)])
            os.kill(os.getpid(), signal.SIGKILL)
        write_all(descriptor, encoded)
        if fault == "after_write_before_fsync_kill":
            os.kill(os.getpid(), signal.SIGKILL)
        self.stream.flush()
        os.fsync(descriptor)
        require(
            stat.S_IMODE(self.path.stat().st_mode) == 0o600,
            "output_mode_invalid",
        )
        self.sequence += 1
        self.previous_record_sha256 = record_sha256
        return record_sha256


def read_journal(path: Path, *, allow_partial_tail: bool) -> dict[str, Any]:
    metadata = path.lstat()
    require(
        stat.S_ISREG(metadata.st_mode)
        and not path.is_symlink()
        and stat.S_IMODE(metadata.st_mode) == 0o600,
        "journal_file_invalid",
    )
    raw = path.read_bytes()
    parts = raw.split(b"\n")
    tail = parts.pop()
    if tail:
        require(allow_partial_tail, "journal_partial_tail")
    records: list[dict[str, Any]] = []
    previous = ZERO_HASH
    for index, line in enumerate(parts, 1):
        require(bool(line), "journal_blank_record")
        try:
            record = json.loads(line)
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise EvaluationError("journal_record_invalid") from error
        core = {
            key: record.get(key)
            for key in (
                "schema",
                "sequence",
                "previous_record_sha256",
                "payload",
            )
        }
        require(
            core["schema"] == JOURNAL_SCHEMA
            and core["sequence"] == index
            and core["previous_record_sha256"] == previous
            and isinstance(core["payload"], dict),
            "journal_chain_invalid",
        )
        require(
            record.get("record_sha256") == canonical_hash(core),
            "journal_hash_invalid",
        )
        previous = record["record_sha256"]
        records.append(record)
    return {
        "records": records,
        "partial_tail_bytes": len(tail),
        "file_sha256": sha256_bytes(raw),
    }


def load_manifest() -> dict[str, Any]:
    try:
        manifest = json.loads(MANIFEST_PATH.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise EvaluationError("manifest_unavailable") from error
    require(
        isinstance(manifest, dict)
        and manifest.get("schema") == MANIFEST_SCHEMA,
        "manifest_schema_invalid",
    )
    source = manifest.get("source_identity", {})
    require(
        source.get("run_api") == RUN_API
        and source.get("runtime_event") == EVENT_API
        and source.get("state_schema") == STATE_SCHEMA
        and source.get("exec_stream") == EXEC_STREAM,
        "protocol_identity_invalid",
    )
    experiment = manifest.get("formal_experiment", {})
    require(
        experiment.get("runs_per_variant_task") == RUNS_PER_CELL
        and experiment.get("formal_arms") == 27
        and list(experiment.get("variants", {})) == list(VARIANTS),
        "experiment_identity_invalid",
    )
    require(
        [task.get("id") for task in experiment.get("tasks", [])]
        == list(TASKS),
        "task_identity_invalid",
    )
    return manifest


def formal_schedule() -> list[dict[str, Any]]:
    task_ids = list(TASKS)
    schedule: list[dict[str, Any]] = []
    for run_index in range(RUNS_PER_CELL):
        rotated_tasks = task_ids[run_index:] + task_ids[:run_index]
        for task_position, task_id in enumerate(rotated_tasks):
            cell_index = run_index * len(task_ids) + task_position
            offset = cell_index % len(VARIANTS)
            order = VARIANTS[offset:] + VARIANTS[:offset]
            for position, variant in enumerate(order):
                schedule.append(
                    {
                        "cell_index": cell_index,
                        "task_id": task_id,
                        "run_index": run_index,
                        "variant": variant,
                        "cell_position": position,
                    }
                )
    return schedule


def fixture_file_sha_chain(path: Path) -> str:
    """Reproduce the frozen `find|sort|shasum|shasum` fixture identity."""
    lines = bytearray()
    for item in sorted(
        candidate for candidate in path.rglob("*") if candidate.is_file()
    ):
        relative = item.relative_to(ROOT).as_posix()
        digest = hashlib.sha256(item.read_bytes()).hexdigest()
        lines.extend(f"{digest}  {relative}\n".encode("utf-8"))
    return hashlib.sha256(lines).hexdigest()


def task_definition(task_id: str) -> dict[str, Any]:
    task = TASKS[task_id]
    first, second = task["partitions"]
    prompt = f"""任务：{task["objective"]}

共同约束：
1. 只允许修改 `{task["target"]}`；不得修改 README、JSON、verifier、Git 元数据或新增文件。
2. 不得读取环境变量、凭据或仓库外路径。
3. 第一次模型回合必须在同一个 assistant response 中恰好调用两次 `agent`；
   两者都使用 `type="explore"`、`fork_context=false`、`max_steps=3`、
   `wall_time_secs=120`、`allowed_tools={json.dumps(M7G.CHILD_TOOLS)}`，且只读分工分别为：
   A. {first}；
   B. {second}。
4. 发出两个 child 调用的同一回合不得调用其他工具。收到两个 typed handoff 后，只有
   root 可以修改 `{task["target"]}`；不得启动第三个 child。
5. 修改后可调用 `run_verifiers` 做 exact 验证；最终完成必须由 Host 基于最新 workspace
   revision 的 deterministic verifier 接受，不得自称测试通过。
"""
    verifier = M7G.verifier_spec(task_id)
    return {
        "objective": prompt,
        "constraints": [
            f"只修改 {task['target']}",
            "同一回合恰好启动两个只读 Explorer",
            "maximum_reruns=0",
        ],
        "non_goals": [
            "Writer child",
            "修改 fixture 或 verifier",
            "读取凭据或仓库外路径",
        ],
        "acceptance": [
            {
                "kind": "verifier",
                "id": f"{task_id}-exact",
                "description": "冻结 fixture 的确定性 verifier 必须通过",
                "evidence_policy": "latest_pass",
                "verifier": verifier,
            }
        ],
    }


def start_envelope(
    task_id: str, variant: str, workspace: Path, request_id: str
) -> dict[str, Any]:
    command: dict[str, Any] = {
        "kind": "start",
        "task": task_definition(task_id),
        "workspace": str(workspace.resolve()),
        "reasoning_effort": "high",
        "max_output_tokens": MAX_OUTPUT_TOKENS,
        "max_api_requests": MAX_API_REQUESTS,
        "streaming": True,
        "tool_policy": {
            "enabled": True,
            "allowed": ROOT_TOOLS,
            "denied": [],
        },
        "limits": {
            "max_turns": MAX_TURNS,
            "max_model_requests": MAX_API_REQUESTS,
            "max_model_retries": 0,
            "max_tool_calls": MAX_TOOL_CALLS,
            "max_depth": 1,
            "max_concurrent_children": 2,
            "model_event_idle_ms": MODEL_IDLE_MS,
            "wall_time_ms": WALL_TIME_SECONDS * 1000,
        },
        "controls": {
            "write_execution_mode": "root",
            "auto_approve": True,
            "trust_mode": False,
            "allow_sandbox_elevation": False,
            "interactive": False,
            "sandbox": "workspace-write",
        },
    }
    model = MODELS[variant]
    if model is not None:
        command["model"] = model
    return {
        "schema_version": RUN_API,
        "request_id": request_id,
        "command": command,
    }


def launch_server(
    binary: Path,
    workspace: Path,
    state_root: Path,
    key: str | None,
    stderr_path: Path,
) -> tuple[subprocess.Popen[bytes], Any]:
    home = state_root / "home"
    codewhale_home = state_root / "codewhale"
    xdg = state_root / "xdg"
    for directory in (state_root, home, codewhale_home, xdg):
        directory.mkdir(parents=True, exist_ok=True)
    environment = {
        **M7G.safe_env(),
        "HOME": str(home),
        "CODEWHALE_HOME": str(codewhale_home),
        "XDG_CONFIG_HOME": str(xdg),
    }
    if key is not None:
        environment["DEEPSEEK_API_KEY"] = key
    stderr_stream = stderr_path.open("ab")
    try:
        process = subprocess.Popen(
            [str(binary), "app-server", "--stdio"],
            cwd=workspace,
            env=environment,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=stderr_stream,
            start_new_session=True,
        )
    except OSError as error:
        stderr_stream.close()
        raise EvaluationError("app_server_launch_failed") from error
    stderr_stream.close()
    forbidden = key.encode("utf-8") if key else b"\0key-not-present\0"
    return process, M7G.StdioClient(process, forbidden)


def event_kind(stored: dict[str, Any]) -> str:
    return M7G.event_kind(stored)


def child_ids(root_events: list[dict[str, Any]]) -> list[str]:
    values = [
        stored.get("event", {}).get("task", {}).get("child_run_id")
        for stored in root_events
        if event_kind(stored) == "agent_task_prepared"
    ]
    require(
        all(isinstance(value, str) and value for value in values),
        "child_id_invalid",
    )
    return values


def fetch_store_facts(
    client: Any, run: dict[str, Any], suffix: str
) -> dict[str, Any]:
    run_id = run.get("run_id")
    require(isinstance(run_id, str) and run_id, "run_id_missing")
    root_events = M7G.fetch_events(client, run_id, f"{suffix}-root")
    children: list[dict[str, Any]] = []
    for index, child_id in enumerate(child_ids(root_events)):
        result = client.call(
            M7G.query_envelope(
                "get", child_id, f"{suffix}-child-{index}"
            )
        )
        require(result.get("kind") == "run", "child_run_missing")
        child = result.get("run")
        require(isinstance(child, dict), "child_run_missing")
        events = M7G.fetch_events(
            client, child_id, f"{suffix}-child-events-{index}"
        )
        children.append({"run": child, "events": events})
    return {
        "run": run,
        "root_events": root_events,
        "children": children,
    }


def route_audit(variant: str, facts: dict[str, Any]) -> dict[str, Any]:
    expected = EXPECTED_ROUTES[variant]
    root_created = [
        stored["event"].get("request")
        for stored in facts["root_events"]
        if event_kind(stored) == "run_created"
    ]
    require(len(root_created) == 1, "root_created_invalid")
    root_request = root_created[0]
    require(isinstance(root_request, dict), "root_request_invalid")
    root_route = root_request.get("route", {})
    reasons: list[str] = []
    if root_request.get("model") != expected["root_model"]:
        reasons.append("root_model_mismatch")
    if root_request.get("reasoning_effort") != "high":
        reasons.append("root_reasoning_mismatch")
    if root_route.get("requested_model_mode") != expected["root_mode"]:
        reasons.append("root_requested_mode_mismatch")
    if root_route.get("policy_version") != expected["policy"]:
        reasons.append("root_policy_mismatch")
    if root_route.get("reason_code") != expected["root_reason"]:
        reasons.append("root_reason_code_mismatch")
    tasks = [
        stored["event"].get("task", {})
        for stored in facts["root_events"]
        if event_kind(stored) == "agent_task_prepared"
    ]
    if len(tasks) != 2:
        reasons.append("exactly_two_child_routes_required")
    for task in tasks:
        route = task.get("route", {})
        if task.get("model") != expected["child_model"]:
            reasons.append("child_task_model_mismatch")
        if task.get("reasoning_effort") != "high":
            reasons.append("child_task_reasoning_mismatch")
        if task.get("workspace", {}).get("access") != "read_only":
            reasons.append("child_workspace_not_read_only")
        if route.get("requested_model_mode") != expected["root_mode"]:
            reasons.append("child_requested_mode_mismatch")
        if route.get("policy_version") != expected["policy"]:
            reasons.append("child_policy_mismatch")
        if route.get("reason_code") != expected["child_reason"]:
            reasons.append("child_reason_code_mismatch")
    root_model_requests = [
        stored["event"].get("request", {})
        for stored in facts["root_events"]
        if event_kind(stored) == "model_request_prepared"
    ]
    if not root_model_requests or any(
        request.get("model") != expected["root_model"]
        or request.get("actor", {}).get("kind") != "root"
        for request in root_model_requests
    ):
        reasons.append("root_model_request_mismatch")
    child_request_counts: list[int] = []
    for child in facts["children"]:
        child_created = [
            stored["event"].get("request")
            for stored in child["events"]
            if event_kind(stored) == "run_created"
        ]
        if len(child_created) != 1 or not isinstance(child_created[0], dict):
            reasons.append("child_run_created_invalid")
            continue
        request = child_created[0]
        route = request.get("route", {})
        if (
            request.get("model") != expected["child_model"]
            or request.get("reasoning_effort") != "high"
            or route.get("reason_code") != expected["child_reason"]
        ):
            reasons.append("child_run_binding_mismatch")
        requests = [
            stored["event"].get("request", {})
            for stored in child["events"]
            if event_kind(stored) == "model_request_prepared"
        ]
        child_request_counts.append(len(requests))
        if not requests or any(
            item.get("model") != expected["child_model"]
            or item.get("actor", {}).get("kind") != "child"
            for item in requests
        ):
            reasons.append("child_model_request_mismatch")
    return {
        "valid": not reasons,
        "reasons": sorted(set(reasons)),
        "root_model": root_request.get("model"),
        "root_route": root_route,
        "child_routes": [
            {
                "model": task.get("model"),
                "reasoning_effort": task.get("reasoning_effort"),
                "workspace_access": task.get("workspace", {}).get("access"),
                "route": task.get("route"),
            }
            for task in tasks
        ],
        "root_model_requests": len(root_model_requests),
        "child_model_requests": child_request_counts,
    }


def lifecycle_audit(facts: dict[str, Any]) -> dict[str, Any]:
    audit = M7G.audit_lifecycle(
        "treatment",
        facts["root_events"],
        [child["run"] for child in facts["children"]],
    )
    positions: dict[str, list[int]] = {}
    for index, stored in enumerate(facts["root_events"]):
        positions.setdefault(event_kind(stored), []).append(index)
    root_requests = positions.get("model_request_prepared", [])
    finished = positions.get("child_finished", [])
    integration_after_handoffs = bool(
        finished
        and any(position > max(finished) for position in root_requests)
    )
    if not integration_after_handoffs:
        audit["valid"] = False
        audit["reasons"].append("root_integration_after_handoffs_missing")
    audit["integration_after_handoffs"] = integration_after_handoffs
    return audit


def accounting_projection(
    variant: str, run: dict[str, Any]
) -> dict[str, Any]:
    accounting = run.get("accounting", {})
    root = accounting.get("root", {})
    child = accounting.get("child", {})
    usage = run.get("usage", {})
    safety = {
        "complete": accounting.get("complete"),
        "usage_complete": accounting.get("usage_complete"),
        "usage_missing": accounting.get("usage_missing"),
        "usage_incomplete": accounting.get("usage_incomplete"),
        "billing_unknown": accounting.get("billing_unknown"),
        "unpriced": accounting.get("unpriced"),
    }
    require(
        accounting.get("hard_request_limit") == MAX_API_REQUESTS
        and safety
        == {
            "complete": True,
            "usage_complete": True,
            "usage_missing": False,
            "usage_incomplete": False,
            "billing_unknown": False,
            "unpriced": False,
        },
        "accounting_incomplete",
        safety,
    )
    started = int(root.get("started", -1)) + int(child.get("started", -1))
    completed = int(root.get("completed", -1)) + int(child.get("completed", -1))
    in_flight = int(root.get("in_flight", -1)) + int(child.get("in_flight", -1))
    require(
        started > 0
        and started == completed
        and in_flight == 0
        and int(accounting.get("transport_retries", -1)) == 0,
        "request_accounting_invalid",
    )
    surfaces = accounting.get("surface_usage")
    require(isinstance(surfaces, list) and surfaces, "surface_usage_missing")
    actual_models = {
        item.get("model")
        for item in surfaces
        if isinstance(item, dict) and item.get("surface") == "standard_chat"
    }
    expected_models = {
        EXPECTED_ROUTES[variant]["root_model"],
        EXPECTED_ROUTES[variant]["child_model"],
    }
    require(actual_models == expected_models, "surface_identity_invalid")
    tokens = {
        "input": int(usage.get("input_tokens", -1)),
        "output": int(usage.get("output_tokens", -1)),
        "cache_hit": int(usage.get("cache_hit_tokens", -1)),
        "cache_miss": int(usage.get("cache_miss_tokens", -1)),
        "reasoning": int(usage.get("reasoning_tokens", -1)),
    }
    require(
        all(value >= 0 for value in tokens.values())
        and tokens["input"] == tokens["cache_hit"] + tokens["cache_miss"],
        "usage_identity_invalid",
    )
    cost_nanousd = int(accounting.get("cost_nanousd", -1))
    cost_nanocny = int(accounting.get("cost_nanocny", -1))
    require(cost_nanousd >= 0 and cost_nanocny >= 0, "cost_identity_invalid")
    return {
        "root_requests": int(root["started"]),
        "child_requests": int(child["started"]),
        "requests": started,
        "transport_retries": 0,
        "tokens": tokens,
        "cost_nanousd": cost_nanousd,
        "cost_nanocny": cost_nanocny,
        "cost_usd": cost_nanousd / 1_000_000_000,
        "surface_usage": surfaces,
    }


def tool_outcome_success(outcome: Any) -> bool:
    return M7G.tool_outcome_success(outcome)


def derive_arm(
    binary_identity: dict[str, Any],
    schedule: dict[str, Any],
    facts: dict[str, Any],
    verifier: dict[str, Any],
    changed: list[str],
    wall_time_ms: int,
    stderr: bytes,
    state_identity: dict[str, Any],
) -> dict[str, Any]:
    variant = schedule["variant"]
    task_id = schedule["task_id"]
    run = facts["run"]
    terminal = run.get("terminal", {})
    accounting = accounting_projection(variant, run)
    route = route_audit(variant, facts)
    lifecycle = lifecycle_audit(facts)
    expected_changed = [TASKS[task_id]["target"]]
    host_receipt = any(
        stored["event"].get("receipt") is not None
        and tool_outcome_success(stored["event"].get("outcome"))
        for stored in facts["root_events"]
        if event_kind(stored) == "host_verification_committed"
    )
    terminal_completed = terminal.get("state") == "completed"
    verified_success = (
        terminal_completed
        and verifier["passed"]
        and changed == expected_changed
        and host_receipt
        and route["valid"]
        and lifecycle["valid"]
    )
    false_success = terminal_completed and not verified_success
    tool_outcomes = [
        stored["event"]
        for stored in facts["root_events"]
        if event_kind(stored) == "tool_outcome_committed"
    ]
    failure_codes = Counter(
        event.get("outcome", {}).get("failure_code")
        or "missing_failure_code"
        for event in tool_outcomes
        if not tool_outcome_success(event.get("outcome"))
    )
    require(
        accounting["cost_usd"] <= PER_ARM_COST_CEILING_USD,
        "arm_cost_ceiling_exceeded",
        {"cost_usd": accounting["cost_usd"]},
    )
    return {
        "record_type": "arm_result",
        **schedule,
        "binary": binary_identity,
        "task_definition_sha256": canonical_hash(task_definition(task_id)),
        "terminal_state": terminal.get("state"),
        "terminal_completed": terminal_completed,
        "verified_success": verified_success,
        "false_success": false_success,
        "external_verifier": verifier,
        "changed_files": changed,
        "expected_changed_files": expected_changed,
        "host_receipt": host_receipt,
        "route": route,
        "lifecycle": lifecycle,
        "accounting": accounting,
        "failed_tool_outcomes": sum(failure_codes.values()),
        "failure_codes": dict(sorted(failure_codes.items())),
        "wall_time_ms": wall_time_ms,
        "stderr_sha256": sha256_bytes(stderr),
        "state_schema": state_identity,
        "key_accessed": True,
        "network_accessed": True,
        "maximum_reruns": 0,
    }


def execute_arm(
    binary: Path,
    binary_identity: dict[str, Any],
    key: str,
    schedule: dict[str, Any],
    journal: Journal,
) -> dict[str, Any]:
    task_id = schedule["task_id"]
    evaluation_id = uuid.uuid4().hex
    started_at = time.monotonic()
    secret = key.encode("utf-8")
    journal.emit(
        {
            "record_type": "arm_started",
            "evaluation_id": evaluation_id,
            **schedule,
            "key_accessed": True,
            "network_accessed": False,
            "maximum_reruns": 0,
        }
    )
    with tempfile.TemporaryDirectory(prefix="codewhale-m9a-arm-") as raw_temp:
        arm_root = Path(raw_temp)
        workspace = arm_root / "workspace"
        fixture_sha256 = M7G.materialize_fixture(task_id, workspace)
        state_root = arm_root / "state"
        stderr_path = state_root / "app-server.stderr"
        state_root.mkdir(parents=True, exist_ok=True)
        process, client = launch_server(
            binary, workspace, state_root, key, stderr_path
        )
        run: dict[str, Any] = {}
        facts: dict[str, Any] = {}
        try:
            result = client.call(
                start_envelope(
                    task_id, schedule["variant"], workspace, f"start-{evaluation_id}"
                ),
                30,
            )
            require(result.get("kind") == "run", "start_run_missing")
            run = result.get("run")
            require(isinstance(run, dict), "start_run_missing")
            deadline = time.monotonic() + WALL_TIME_SECONDS + HARNESS_GRACE_SECONDS
            run = M7G.wait_terminal(client, run, deadline, evaluation_id)
            journal.emit(
                {
                    "record_type": "terminal_snapshot",
                    "evaluation_id": evaluation_id,
                    "run": run,
                    "run_sha256": canonical_hash(run),
                    "key_accessed": True,
                    "network_accessed": True,
                }
            )
            facts = fetch_store_facts(client, run, evaluation_id)
            journal.emit(
                {
                    "record_type": "canonical_store_snapshot",
                    "evaluation_id": evaluation_id,
                    "facts": facts,
                    "facts_sha256": canonical_hash(facts),
                    "key_accessed": True,
                    "network_accessed": True,
                }
            )
        finally:
            M7G.stop_process(process)

        stderr = stderr_path.read_bytes() if stderr_path.exists() else b""
        require(secret not in stderr, "key_in_stderr")
        require(secret not in canonical_bytes(facts), "key_in_run_projection")
        require(not M7G.tree_contains(arm_root, secret), "key_in_local_artifact")

        reopen_stderr = state_root / "app-server-reopen.stderr"
        reopen_process, reopen_client = launch_server(
            binary, workspace, state_root, None, reopen_stderr
        )
        try:
            run_id = run.get("run_id")
            require(isinstance(run_id, str), "run_id_missing")
            reopened_result = reopen_client.call(
                M7G.query_envelope("get", run_id, f"reopen-{evaluation_id}")
            )
            require(reopened_result.get("kind") == "run", "reopen_run_missing")
            reopened_run = reopened_result.get("run")
            require(isinstance(reopened_run, dict), "reopen_run_missing")
            reopened = fetch_store_facts(
                reopen_client, reopened_run, f"reopen-{evaluation_id}"
            )
        finally:
            M7G.stop_process(reopen_process)
        reopen_stderr_bytes = (
            reopen_stderr.read_bytes() if reopen_stderr.exists() else b""
        )
        require(secret not in reopen_stderr_bytes, "key_in_reopen_stderr")
        require(facts == reopened, "sqlite_reopen_mismatch")
        journal.emit(
            {
                "record_type": "sqlite_reopen_snapshot",
                "evaluation_id": evaluation_id,
                "facts": reopened,
                "facts_sha256": canonical_hash(reopened),
                "matches_terminal_store_snapshot": True,
                "reopened_without_credential": True,
                "key_accessed": True,
                "network_accessed": True,
            }
        )

        verifier = M7G.external_verifier(workspace)
        changed = M7G.changed_files(workspace)
        journal.emit(
            {
                "record_type": "verifier_snapshot",
                "evaluation_id": evaluation_id,
                "verifier": verifier,
                "changed_files": changed,
                "key_accessed": True,
                "network_accessed": True,
            }
        )
        state_identity = M7G.state_schema(state_root / "codewhale")
        arm = derive_arm(
            binary_identity,
            schedule,
            facts,
            verifier,
            changed,
            int((time.monotonic() - started_at) * 1000),
            stderr + reopen_stderr_bytes,
            state_identity,
        )
        arm["evaluation_id"] = evaluation_id
        arm["fixture_sha256"] = fixture_sha256
        journal.emit(arm)
        return arm


def paired_results(
    arms: list[dict[str, Any]], metric: str
) -> list[dict[str, Any]]:
    cells: dict[tuple[str, int], dict[str, dict[str, Any]]] = {}
    for arm in arms:
        cells.setdefault((arm["task_id"], arm["run_index"]), {})[
            arm["variant"]
        ] = arm
    require(
        len(cells) == 9
        and all(set(cell) == set(VARIANTS) for cell in cells.values()),
        "formal_matrix_incomplete",
    )
    results = []
    for (task_id, run_index), cell in sorted(cells.items()):
        pro = cell["fixed_pro"]
        auto = cell["host_auto"]
        pro_value = (
            pro["accounting"][metric]
            if metric == "cost_nanousd"
            else pro[metric]
        )
        auto_value = (
            auto["accounting"][metric]
            if metric == "cost_nanousd"
            else auto[metric]
        )
        results.append(
            {
                "task_id": task_id,
                "run_index": run_index,
                "fixed_pro": pro_value,
                "host_auto": auto_value,
                "delta": auto_value - pro_value,
                "fixed_pro_verified": pro["verified_success"],
                "host_auto_verified": auto["verified_success"],
            }
        )
    return results


def aggregate(arms: list[dict[str, Any]]) -> dict[str, Any]:
    variants: dict[str, Any] = {}
    for variant in VARIANTS:
        selected = [arm for arm in arms if arm["variant"] == variant]
        variants[variant] = {
            "arms": len(selected),
            "verified_success": sum(arm["verified_success"] for arm in selected),
            "false_success": sum(arm["false_success"] for arm in selected),
            "route_valid": sum(arm["route"]["valid"] for arm in selected),
            "lifecycle_valid": sum(
                arm["lifecycle"]["valid"] for arm in selected
            ),
            "cost_nanousd_total": sum(
                arm["accounting"]["cost_nanousd"] for arm in selected
            ),
            "wall_time_ms_total": sum(arm["wall_time_ms"] for arm in selected),
        }
    task_quality = {}
    for task_id in TASKS:
        task_quality[task_id] = {
            variant: sum(
                arm["verified_success"]
                for arm in arms
                if arm["task_id"] == task_id and arm["variant"] == variant
            )
            for variant in VARIANTS
        }
    cost_pairs = paired_results(arms, "cost_nanousd")
    wall_pairs = paired_results(arms, "wall_time_ms")
    pro = variants["fixed_pro"]
    auto = variants["host_auto"]
    quality_pass = (
        auto["false_success"] == 0
        and auto["route_valid"] == 9
        and auto["lifecycle_valid"] == 9
        and all(
            values["host_auto"] >= values["fixed_pro"]
            for values in task_quality.values()
        )
        and not any(
            pair["fixed_pro_verified"] and not pair["host_auto_verified"]
            for pair in cost_pairs
        )
    )
    cost_ratio = (
        auto["cost_nanousd_total"] / pro["cost_nanousd_total"]
        if pro["cost_nanousd_total"]
        else None
    )
    wall_ratio = (
        auto["wall_time_ms_total"] / pro["wall_time_ms_total"]
        if pro["wall_time_ms_total"]
        else None
    )
    stable_cost = (
        cost_ratio is not None
        and cost_ratio <= 0.8
        and sum(pair["delta"] < 0 for pair in cost_pairs) >= 6
        and statistics.median(pair["delta"] for pair in cost_pairs) < 0
    )
    stable_wall = (
        wall_ratio is not None
        and wall_ratio <= 0.8
        and sum(pair["delta"] < 0 for pair in wall_pairs) >= 6
        and statistics.median(pair["delta"] for pair in wall_pairs) < 0
    )
    decision = (
        "admit_default_auto"
        if quality_pass and (stable_cost or stable_wall)
        else "hold_fixed_pro_default"
    )
    return {
        "record_type": "summary",
        "product_metric_eligible": True,
        "variants": variants,
        "task_quality": task_quality,
        "cost_pairs": cost_pairs,
        "wall_pairs": wall_pairs,
        "quality_pass": quality_pass,
        "cost_ratio_auto_over_pro": cost_ratio,
        "wall_ratio_auto_over_pro": wall_ratio,
        "stable_cost_gain_at_least_20_percent": stable_cost,
        "stable_wall_gain_at_least_20_percent": stable_wall,
        "decision": decision,
        "key_accessed": True,
        "network_accessed": True,
        "maximum_reruns": 0,
    }


def probe_binary(binary: Path, revision: str) -> dict[str, Any]:
    require(binary.is_file() and not binary.is_symlink(), "binary_unavailable")
    result = M7G.run_command([str(binary), "--version"], cwd=ROOT, timeout=15)
    require(result.returncode == 0, "binary_probe_failed")
    try:
        version = result.stdout.decode("utf-8").strip()
    except UnicodeDecodeError as error:
        raise EvaluationError("binary_probe_failed") from error
    require(revision[:12] in version, "binary_revision_mismatch")
    return {
        "revision": revision,
        "sha256": file_hash(binary),
        "size_bytes": binary.stat().st_size,
        "version": version,
    }


def load_admission(path: Path, revision: str, binary: Path) -> dict[str, Any]:
    try:
        admission = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise EvaluationError("live_admission_unavailable") from error
    require(
        admission.get("schema") == ADMISSION_SCHEMA
        and admission.get("candidate_revision") == revision
        and admission.get("candidate_binary_sha256") == file_hash(binary)
        and admission.get("offline_gates_passed") is True
        and admission.get("live_api_admitted") is True,
        "live_admission_invalid",
    )
    return admission


def preflight(
    binary: Path,
    revision: str,
    admission_path: Path | None,
    *,
    formal: bool,
) -> dict[str, Any]:
    require(
        git_output("branch", "--show-current") == "deepseek-agent",
        "branch_invalid",
    )
    require(not git_output("status", "--porcelain=v1"), "worktree_dirty")
    manifest = load_manifest()
    identity = probe_binary(binary, revision)
    expected_hashes = {
        task["id"]: task["fixture_tree_sha256"]
        for task in manifest["formal_experiment"]["tasks"]
    }
    actual_hashes = {
        task_id: fixture_file_sha_chain(FIXTURE_ROOT / task_id)
        for task_id in TASKS
    }
    require(actual_hashes == expected_hashes, "fixture_hash_mismatch")
    admission = None
    if formal:
        require(admission_path is not None, "live_admission_required")
        admission = load_admission(admission_path, revision, binary)
    return {
        "manifest_sha256": file_hash(MANIFEST_PATH),
        "harness_sha256": file_hash(Path(__file__).resolve()),
        "schedule_sha256": canonical_hash(formal_schedule()),
        "task_contracts_sha256": canonical_hash(
            {task_id: task_definition(task_id) for task_id in TASKS}
        ),
        "fixture_hashes": actual_hashes,
        "binary": identity,
        "admission_sha256": (
            file_hash(admission_path)
            if admission is not None and admission_path is not None
            else None
        ),
    }


def read_key(path: Path) -> str:
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(path, flags)
    except OSError as error:
        raise EvaluationError("key_unavailable") from error
    try:
        metadata = os.fstat(descriptor)
        require(stat.S_ISREG(metadata.st_mode), "key_not_regular")
        require(stat.S_IMODE(metadata.st_mode) == 0o600, "key_mode_invalid")
        require(0 < metadata.st_size <= 16_384, "key_size_invalid")
        raw = os.read(descriptor, 16_385)
        require(len(raw) == metadata.st_size, "key_size_changed")
        value = raw.decode("utf-8").strip()
    except UnicodeDecodeError as error:
        raise EvaluationError("key_encoding_invalid") from error
    finally:
        os.close(descriptor)
    require(
        bool(value)
        and not any(character.isspace() or ord(character) < 32 for character in value),
        "key_format_invalid",
    )
    return value


def plan_record(identity: dict[str, Any]) -> dict[str, Any]:
    return {
        "record_type": "plan",
        "product_metric_eligible": False,
        "source_identity": identity,
        "variants": {
            variant: {
                "requested_model": MODELS[variant] or "auto",
                "requested_reasoning": "high",
                "agent_advertised": True,
                "required_read_only_children": 2,
            }
            for variant in VARIANTS
        },
        "same_across_arms": {
            "binary": identity["binary"],
            "reasoning_effort": "high",
            "max_output_tokens": MAX_OUTPUT_TOKENS,
            "max_api_requests": MAX_API_REQUESTS,
            "max_turns": MAX_TURNS,
            "max_tool_calls": MAX_TOOL_CALLS,
            "maximum_reruns": 0,
        },
        "schedule": formal_schedule(),
        "arms": 27,
        "suite_cost_ceiling_usd": 27 * PER_ARM_COST_CEILING_USD,
        "key_accessed": False,
        "network_accessed": False,
    }


def run_fault_child(fault: str, output: Path, *, self_test: bool) -> int:
    with Journal.claim(output, enforce_results_scope=not self_test) as journal:
        journal.emit(
            {
                "record_type": "plan",
                "key_accessed": False,
                "network_accessed": False,
            }
        )
        if fault == "before_terminal":
            os.kill(os.getpid(), signal.SIGKILL)
        journal.emit(
            {
                "record_type": "terminal_snapshot",
                "run": {"terminal": {"state": "completed"}},
                "key_accessed": False,
                "network_accessed": False,
            },
            fault=(
                "mid_write_kill"
                if fault == "mid_terminal"
                else (
                    "after_write_before_fsync_kill"
                    if fault == "unfsynced_terminal"
                    else None
                )
            ),
        )
        if fault == "after_terminal":
            os.kill(os.getpid(), signal.SIGKILL)
    return 0


def run_self_test() -> int:
    manifest = load_manifest()
    schedule = formal_schedule()
    require(len(schedule) == 27, "self_test_schedule_length")
    require(
        Counter(item["variant"] for item in schedule)
        == Counter({variant: 9 for variant in VARIANTS}),
        "self_test_schedule_balance",
    )
    expected_hashes = {
        task["id"]: task["fixture_tree_sha256"]
        for task in manifest["formal_experiment"]["tasks"]
    }
    require(
        {
            task_id: fixture_file_sha_chain(FIXTURE_ROOT / task_id)
            for task_id in TASKS
        }
        == expected_hashes,
        "self_test_fixture_identity",
    )
    require(
        "model" not in start_envelope(
            list(TASKS)[0], "host_auto", ROOT, "self-test"
        )["command"],
        "self_test_auto_must_omit_model",
    )
    faults = (
        "before_terminal",
        "mid_terminal",
        "unfsynced_terminal",
        "after_terminal",
    )
    fault_results = []
    with tempfile.TemporaryDirectory(
        prefix="codewhale-m9a-journal-test-"
    ) as raw_temp:
        directory = Path(raw_temp)
        for fault in faults:
            output = directory / f"{fault}.jsonl"
            completed = subprocess.run(
                [
                    str(Path(sys.executable).resolve()),
                    "-I",
                    "-B",
                    str(Path(__file__).resolve()),
                    "--fault-child",
                    fault,
                    "--output",
                    str(output),
                    "--self-test-fault",
                ],
                cwd=ROOT,
                env=M7G.safe_env(),
                stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=30,
                check=False,
            )
            require(completed.returncode == -signal.SIGKILL, "fault_exit_invalid")
            audit = read_journal(output, allow_partial_tail=True)
            types = [
                record["payload"].get("record_type")
                for record in audit["records"]
            ]
            require(types[0] == "plan", "fault_plan_missing")
            if fault == "before_terminal":
                require(types == ["plan"], "fault_terminal_order_invalid")
            elif fault == "mid_terminal":
                require(
                    types == ["plan"] and audit["partial_tail_bytes"] > 0,
                    "fault_partial_tail_invalid",
                )
            else:
                require(
                    "terminal_snapshot" in types,
                    "fault_terminal_snapshot_missing",
                )
            fault_results.append(
                {
                    "fault": fault,
                    "record_types": types,
                    "partial_tail_bytes": audit["partial_tail_bytes"],
                }
            )
        complete = directory / "complete.jsonl"
        with Journal.claim(complete, enforce_results_scope=False) as journal:
            journal.emit(
                {
                    "record_type": "plan",
                    "key_accessed": False,
                    "network_accessed": False,
                }
            )
            journal.emit(
                {
                    "record_type": "terminal_snapshot",
                    "run": {"terminal": {"state": "completed"}},
                    "key_accessed": False,
                    "network_accessed": False,
                }
            )
        lines = complete.read_bytes().splitlines()
        tampered = directory / "tampered.jsonl"
        value = json.loads(lines[1])
        value["payload"]["run"]["terminal"]["state"] = "failed"
        tampered.write_bytes(lines[0] + b"\n" + canonical_bytes(value) + b"\n")
        os.chmod(tampered, 0o600)
        try:
            read_journal(tampered, allow_partial_tail=False)
        except EvaluationError as error:
            require(error.code == "journal_hash_invalid", "tamper_rejection_invalid")
        else:
            raise EvaluationError("journal_tamper_accepted")
    print(
        json.dumps(
            {
                "schema": JOURNAL_SCHEMA,
                "record_type": "self_test",
                "passed": True,
                "schedule_sha256": canonical_hash(schedule),
                "task_contracts_sha256": canonical_hash(
                    {
                        task_id: task_definition(task_id)
                        for task_id in TASKS
                    }
                ),
                "fault_results": fault_results,
                "key_accessed": False,
                "network_accessed": False,
            },
            ensure_ascii=False,
            sort_keys=True,
        )
    )
    return 0


def run_freeze_report() -> int:
    print(
        json.dumps(
            {
                "harness_sha256": file_hash(Path(__file__).resolve()),
                "schedule_sha256": canonical_hash(formal_schedule()),
                "task_contracts_sha256": canonical_hash(
                    {
                        task_id: task_definition(task_id)
                        for task_id in TASKS
                    }
                ),
                "fixture_hashes": {
                    task_id: fixture_file_sha_chain(FIXTURE_ROOT / task_id)
                    for task_id in TASKS
                },
            },
            ensure_ascii=False,
            indent=2,
            sort_keys=True,
        )
    )
    return 0


def run_dry(args: argparse.Namespace) -> int:
    revision = args.revision or git_output("rev-parse", "HEAD")
    identity = preflight(
        Path(args.binary).resolve(), revision, None, formal=False
    )
    print(json.dumps(plan_record(identity), ensure_ascii=False, sort_keys=True))
    return 0


def run_formal(args: argparse.Namespace) -> int:
    require(args.acknowledge_cost, "cost_acknowledgement_required")
    require(args.key_file, "key_file_required")
    require(args.output, "output_required")
    require(args.admission, "live_admission_required")
    revision = args.revision or git_output("rev-parse", "HEAD")
    binary = Path(args.binary).resolve()
    identity = preflight(
        binary, revision, Path(args.admission).resolve(), formal=True
    )
    with Journal.claim(Path(args.output).resolve()) as journal:
        journal.emit(plan_record(identity))
        key = read_key(Path(args.key_file).expanduser().resolve())
        journal.emit(
            {
                "record_type": "credential_access",
                "key_accessed": True,
                "network_accessed": False,
            }
        )
        frozen_root = Path(tempfile.mkdtemp(prefix="codewhale-m9a-binary-"))
        frozen_binary = frozen_root / "codewhale"
        arms: list[dict[str, Any]] = []
        try:
            shutil.copy2(binary, frozen_binary)
            os.chmod(frozen_binary, 0o500)
            frozen_identity = probe_binary(frozen_binary, revision)
            require(
                frozen_identity["sha256"] == identity["binary"]["sha256"],
                "frozen_binary_mismatch",
            )
            for scheduled in formal_schedule():
                try:
                    arm = execute_arm(
                        frozen_binary,
                        frozen_identity,
                        key,
                        scheduled,
                        journal,
                    )
                except EvaluationError as error:
                    journal.emit(
                        {
                            "record_type": "abort",
                            "error_code": error.code,
                            "details": error.details,
                            "completed_arms": len(arms),
                            "key_accessed": True,
                            "network_accessed": True,
                            "maximum_reruns": 0,
                        }
                    )
                    return 2
                arms.append(arm)
            journal.emit(aggregate(arms))
            return 0
        finally:
            key = ""
            frozen_binary.unlink(missing_ok=True)
            try:
                frozen_root.rmdir()
            except OSError:
                pass


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--self-test", action="store_true")
    mode.add_argument("--freeze-report", action="store_true")
    mode.add_argument("--dry-run", action="store_true")
    parser.add_argument("--fault-child")
    parser.add_argument("--self-test-fault", action="store_true")
    parser.add_argument("--binary")
    parser.add_argument("--revision")
    parser.add_argument("--admission")
    parser.add_argument("--acknowledge-cost", action="store_true")
    parser.add_argument("--key-file")
    parser.add_argument("--output")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        if args.fault_child:
            require(args.output, "output_required")
            return run_fault_child(
                args.fault_child,
                Path(args.output).resolve(),
                self_test=args.self_test_fault,
            )
        if args.self_test:
            return run_self_test()
        if args.freeze_report:
            return run_freeze_report()
        require(args.binary, "binary_required")
        if args.dry_run:
            return run_dry(args)
        return run_formal(args)
    except EvaluationError as error:
        print(
            json.dumps(
                {
                    "schema": JOURNAL_SCHEMA,
                    "record_type": "error",
                    "error_code": error.code,
                    "details": error.details,
                },
                ensure_ascii=False,
                sort_keys=True,
            ),
            file=sys.stderr,
        )
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
