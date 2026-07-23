#!/usr/bin/env python3
"""M7-E same-binary DeepSeek thinking high/off admission evaluator.

The harness speaks current Run API v10 and projects canonical RuntimeEvent v16
facts.  It deliberately does not import an older evaluator, emulate a tool, or
classify failures outside the typed production event ledger.
"""

from __future__ import annotations

import argparse
from collections import Counter, defaultdict
from fractions import Fraction
import hashlib
import json
import os
from pathlib import Path
import selectors
import shutil
import signal
import sqlite3
import stat
import statistics
import subprocess
import sys
import tempfile
import time
from typing import Any
import uuid


ROOT = Path(__file__).resolve().parents[1]
MANIFEST_PATH = ROOT / "eval/manifests/m7-e-thinking-admission-v2.json"
TASK_SOURCE_PATH = ROOT / "eval/manifests/m7-a2-agent-convergence-ab-v1.json"
TEST_PATH = ROOT / "scripts/test-eval-m7e-thinking.py"
SCHEMA = "codewhale.eval.m7-e-thinking-admission.v2"
RESULT_SCHEMA = "codewhale.eval.m7-e-thinking-result.v2"
RUN_API = 10
EVENT_API = 16
STATE_SCHEMA = 21
VARIANTS = ("reasoning_high", "reasoning_off")
MAX_FRAME = 16 * 1024 * 1024
OUTPUT_TAIL_BYTES = 65_536
SAFE_ENV_NAMES = (
    "PATH",
    "TMPDIR",
    "TMP",
    "TEMP",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "HTTPS_PROXY",
    "HTTP_PROXY",
    "ALL_PROXY",
    "NO_PROXY",
    "https_proxy",
    "http_proxy",
    "all_proxy",
    "no_proxy",
)


class EvaluationError(RuntimeError):
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
    return sha256_bytes(path.read_bytes())


def canonical_hash(value: Any) -> str:
    return sha256_bytes(canonical_bytes(value))


def load_json(path: Path, code: str) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise EvaluationError(code) from error
    require(isinstance(value, dict), code)
    return value


def manifest_content_hash(manifest: dict[str, Any]) -> str:
    value = dict(manifest)
    value.pop("frozen_hashes", None)
    return canonical_hash(value)


def load_manifest(*, frozen: bool) -> tuple[dict[str, Any], dict[str, Any]]:
    manifest = load_json(MANIFEST_PATH, "manifest_unavailable")
    require(manifest.get("schema") == SCHEMA, "manifest_schema_mismatch")
    source = manifest.get("source_identity", {})
    require(
        source.get("run_api") == RUN_API
        and source.get("runtime_event") == EVENT_API
        and source.get("state_schema") == STATE_SCHEMA
        and source.get("exec_stream") == 2,
        "manifest_protocol_identity_invalid",
    )
    task_source = manifest.get("task_source", {})
    require(
        task_source.get("manifest") == TASK_SOURCE_PATH.relative_to(ROOT).as_posix()
        and task_source.get("sha256") == file_hash(TASK_SOURCE_PATH),
        "task_source_identity_mismatch",
    )
    tasks = load_json(TASK_SOURCE_PATH, "task_source_unavailable")
    task_ids = task_source.get("task_ids")
    require(
        task_ids == ["t1", "t2", "t3", "t4", "t5"]
        and all(task_id in tasks.get("tasks", {}) for task_id in task_ids),
        "task_source_contract_invalid",
    )
    experiment = manifest.get("experiment", {})
    require(
        experiment.get("variants")
        == {"reasoning_high": "high", "reasoning_off": "off"}
        and experiment.get("runs_per_variant_task") == 3
        and experiment.get("formal_pairs") == 15
        and experiment.get("formal_arms") == 30
        and experiment.get("maximum_reruns") == 0,
        "experiment_contract_invalid",
    )
    resources = manifest.get("resources", {})
    require(
        resources.get("cargo_incremental") == "0"
        and resources.get("cargo_target_dir") == "/private/tmp/codewhale-m7e-target"
        and resources.get("transport_max_retries_per_request") == 0
        and resources.get("max_runtime_retries_per_arm") == 0,
        "resource_contract_invalid",
    )
    output = manifest.get("output", {})
    require(
        output.get("directory") == "eval/raw"
        and output.get("mode") == "0600"
        and output.get("replace") is False
        and output.get("reservation_before_key") is True,
        "output_contract_invalid",
    )
    if frozen:
        hashes = manifest.get("frozen_hashes", {})
        require(
            hashes.get("harness_sha256") == file_hash(Path(__file__).resolve())
            and hashes.get("harness_test_sha256") == file_hash(TEST_PATH)
            and hashes.get("schedule_sha256") == canonical_hash(formal_schedule(manifest))
            and hashes.get("manifest_content_sha256_excluding_frozen_hashes")
            == manifest_content_hash(manifest),
            "frozen_hash_mismatch",
        )
    return manifest, tasks


def formal_schedule(manifest: dict[str, Any]) -> list[dict[str, Any]]:
    task_ids = manifest["task_source"]["task_ids"]
    schedule: list[dict[str, Any]] = []
    rounds = manifest["experiment"]["round_order"]
    require(len(rounds) == 3, "schedule_round_count_invalid")
    for run_index, task_order in enumerate(rounds, start=1):
        require(
            sorted(task_order) == sorted(task_ids) and len(task_order) == len(set(task_order)),
            "schedule_task_set_invalid",
        )
        for task_id in task_order:
            ordinal = task_ids.index(task_id) + 1
            first = (
                "reasoning_high"
                if (run_index + ordinal) % 2 == 0
                else "reasoning_off"
            )
            second = (
                "reasoning_off" if first == "reasoning_high" else "reasoning_high"
            )
            for arm_position, variant in enumerate((first, second), start=1):
                schedule.append(
                    {
                        "task_id": task_id,
                        "run_index": run_index,
                        "variant": variant,
                        "arm_position": arm_position,
                    }
                )
    require(len(schedule) == 30, "schedule_arm_count_invalid")
    require(
        len(
            {
                (arm["task_id"], arm["run_index"], arm["variant"])
                for arm in schedule
            }
        )
        == 30,
        "schedule_duplicate_arm",
    )
    return schedule


def safe_env() -> dict[str, str]:
    environment = {
        name: value
        for name in SAFE_ENV_NAMES
        if (value := os.environ.get(name)) is not None
    }
    for name in list(environment):
        require("KEY" not in name.upper() and "TOKEN" not in name.upper(), "unsafe_env_name")
    return environment


def run_command(
    arguments: list[str],
    *,
    cwd: Path,
    environment: dict[str, str] | None = None,
    timeout: int = 60,
) -> subprocess.CompletedProcess[bytes]:
    try:
        return subprocess.run(
            arguments,
            cwd=cwd,
            env=environment or safe_env(),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
            timeout=timeout,
        )
    except subprocess.TimeoutExpired as error:
        raise EvaluationError(
            "command_timeout",
            {"argv_sha256": canonical_hash(arguments)},
        ) from error


def git_output(*arguments: str, cwd: Path = ROOT) -> str:
    result = run_command(["git", *arguments], cwd=cwd)
    require(
        result.returncode == 0,
        "git_failed",
        {"argv_sha256": canonical_hash(list(arguments)), "returncode": result.returncode},
    )
    return result.stdout.decode("utf-8", errors="strict").rstrip()


def snapshot_tree(root: Path) -> list[dict[str, Any]]:
    entries: list[dict[str, Any]] = []
    for path in sorted(root.rglob("*")):
        relative = path.relative_to(root)
        if ".git" in relative.parts or not path.is_file():
            continue
        metadata = path.lstat()
        require(
            stat.S_ISREG(metadata.st_mode) and not path.is_symlink(),
            "fixture_shape_invalid",
        )
        entries.append(
            {
                "path": relative.as_posix(),
                "mode": stat.S_IMODE(metadata.st_mode),
                "sha256": file_hash(path),
            }
        )
    return entries


def fixture_path(tasks: dict[str, Any], task_id: str) -> Path:
    return ROOT / tasks["tasks"][task_id]["fixture"]


def fixture_hash(tasks: dict[str, Any], task_id: str) -> str:
    return canonical_hash(snapshot_tree(fixture_path(tasks, task_id)))


def materialize_fixture(tasks: dict[str, Any], task_id: str, destination: Path) -> str:
    frozen = tasks["tasks"][task_id]
    require(
        fixture_hash(tasks, task_id) == frozen["fixture_tree_sha256"],
        "fixture_hash_mismatch",
    )
    shutil.copytree(fixture_path(tasks, task_id), destination)
    environment = safe_env()
    environment.update(
        {
            "GIT_AUTHOR_DATE": "2026-07-22T00:00:00Z",
            "GIT_COMMITTER_DATE": "2026-07-22T00:00:00Z",
        }
    )
    commands = (
        ["git", "init", "-q", "-b", "main"],
        ["git", "add", "--", "."],
        [
            "git",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "user.name=CodeWhale Eval",
            "-c",
            "user.email=eval.invalid",
            "commit",
            "-q",
            "-m",
            f"M7-A frozen fixture {fixture_path(tasks, task_id).name}",
        ],
    )
    for command in commands:
        result = run_command(command, cwd=destination, environment=environment)
        require(result.returncode == 0, "fixture_git_init_failed")
    base = git_output("rev-parse", "HEAD", cwd=destination)
    require(
        base == frozen["fixture_base_commit"]
        and git_output(
            "status",
            "--porcelain=v1",
            "--untracked-files=all",
            cwd=destination,
        )
        == "",
        "fixture_git_identity_mismatch",
    )
    return base


def verifier_spec(tasks: dict[str, Any], task_id: str) -> dict[str, Any]:
    acceptance = tasks["tasks"][task_id]["acceptance_id"]
    name = f"{acceptance}-exact"
    command = {
        "name": name,
        "program": "/usr/bin/python3",
        "args": ["-I", "-B", "_eval_verifier.py", "."],
        "cwd": "",
    }
    return {
        "verifier_id": "run_verifiers",
        "parameters": {
            "profile": "exact",
            "level": "quick",
            "max_python_files": 200,
            "commands": [command],
        },
        "plan": {
            "steps": [
                {
                    "id": name,
                    "program": command["program"],
                    "args": command["args"],
                    "cwd": "",
                    "env": {},
                    "timeout_ms": 600_000,
                }
            ]
        },
    }


def task_definition(tasks: dict[str, Any], task_id: str) -> dict[str, Any]:
    frozen = tasks["tasks"][task_id]
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
                "verifier": verifier_spec(tasks, task_id),
            }
        ],
    }


def start_envelope(
    manifest: dict[str, Any],
    tasks: dict[str, Any],
    task_id: str,
    variant: str,
    workspace: Path,
    request_id: str,
) -> dict[str, Any]:
    resources = manifest["resources"]
    task = tasks["tasks"][task_id]
    return {
        "schema_version": RUN_API,
        "request_id": request_id,
        "command": {
            "kind": "start",
            "task": task_definition(tasks, task_id),
            "workspace": str(workspace.resolve()),
            "model": manifest["experiment"]["model"],
            "reasoning_effort": manifest["experiment"]["variants"][variant],
            "max_output_tokens": resources["max_output_tokens_per_request"],
            "max_api_requests": resources["max_physical_api_attempts_per_arm"],
            "streaming": resources["streaming"],
            "tool_policy": {
                "enabled": True,
                "allowed": tasks["tool_policy"]["root_tools"],
                "denied": [],
            },
            "limits": {
                "max_turns": resources["max_logical_model_requests_per_arm"],
                "max_model_requests": resources["max_logical_model_requests_per_arm"],
                "max_model_retries": resources["max_runtime_retries_per_arm"],
                "max_tool_calls": resources["max_tool_calls_per_arm"],
                "max_depth": task["max_depth"],
                "max_concurrent_children": task["max_concurrent_children"],
                "model_event_idle_ms": 120000,
                "wall_time_ms": resources["runtime_wall_time_seconds"] * 1000,
            },
            "controls": {
                "write_execution_mode": "root",
                "auto_approve": resources["auto_approve"],
                "trust_mode": resources["trust_mode"],
                "allow_sandbox_elevation": resources["allow_sandbox_elevation"],
                "interactive": resources["interactive"],
                "sandbox": resources["sandbox"],
            },
        },
    }


def query_envelope(kind: str, run_id: str, request_id: str) -> dict[str, Any]:
    command: dict[str, Any] = {"kind": kind, "run_id": run_id}
    if kind == "events":
        command["after_sequence"] = 0
    return {
        "schema_version": RUN_API,
        "request_id": request_id,
        "command": command,
    }


class StdioClient:
    def __init__(self, process: subprocess.Popen[bytes], forbidden: bytes) -> None:
        require(process.stdin is not None and process.stdout is not None, "stdio_missing")
        self.process = process
        self.stdin_fd = process.stdin.fileno()
        self.stdout_fd = process.stdout.fileno()
        self.forbidden = forbidden
        os.set_blocking(self.stdin_fd, False)
        os.set_blocking(self.stdout_fd, False)
        self.buffer = bytearray()
        self.selector = selectors.DefaultSelector()
        self.selector.register(self.stdout_fd, selectors.EVENT_READ)

    def call(self, envelope: dict[str, Any], timeout_seconds: float) -> dict[str, Any]:
        encoded = canonical_bytes(envelope) + b"\n"
        require(self.forbidden not in encoded, "credential_in_protocol")
        deadline = time.monotonic() + max(0.0, timeout_seconds)
        self._write(encoded, deadline)
        line = self._read(deadline)
        require(self.forbidden not in line, "credential_in_protocol")
        try:
            response = json.loads(line)
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise EvaluationError("stdio_json_invalid") from error
        require(
            isinstance(response, dict)
            and response.get("schema_version") == RUN_API
            and response.get("request_id") == envelope["request_id"]
            and isinstance(response.get("result"), dict),
            "stdio_response_invalid",
        )
        result = response["result"]
        if result.get("kind") == "error":
            error = result.get("error", {})
            code = error.get("code", "unknown") if isinstance(error, dict) else "unknown"
            raise EvaluationError(f"run_api_{code}")
        return result

    def _write(self, value: bytes, deadline: float) -> None:
        remaining = memoryview(value)
        while remaining:
            try:
                count = os.write(self.stdin_fd, remaining)
            except BlockingIOError:
                count = 0
            except OSError as error:
                raise EvaluationError("stdio_write_failed") from error
            if count:
                remaining = remaining[count:]
                continue
            timeout = deadline - time.monotonic()
            require(timeout > 0, "stdio_timeout")
            with selectors.DefaultSelector() as writable:
                writable.register(self.stdin_fd, selectors.EVENT_WRITE)
                require(bool(writable.select(timeout)), "stdio_timeout")

    def _read(self, deadline: float) -> bytes:
        while True:
            newline = self.buffer.find(b"\n")
            if newline >= 0:
                line = bytes(self.buffer[: newline + 1])
                del self.buffer[: newline + 1]
                require(len(line) <= MAX_FRAME, "stdio_frame_too_large")
                return line
            require(len(self.buffer) <= MAX_FRAME, "stdio_frame_too_large")
            timeout = deadline - time.monotonic()
            require(timeout > 0 and bool(self.selector.select(timeout)), "stdio_timeout")
            try:
                chunk = os.read(self.stdout_fd, 65536)
            except BlockingIOError:
                continue
            except OSError as error:
                raise EvaluationError("stdio_read_failed") from error
            require(bool(chunk), "stdio_closed")
            self.buffer.extend(chunk)

    def close(self) -> None:
        self.selector.close()


def stop_process(process: subprocess.Popen[bytes]) -> None:
    if process.poll() is not None:
        return
    try:
        os.killpg(process.pid, signal.SIGTERM)
        process.wait(timeout=5)
    except (ProcessLookupError, subprocess.TimeoutExpired):
        if process.poll() is None:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.wait(timeout=5)


def remaining(deadline: float) -> float:
    value = deadline - time.monotonic()
    require(value > 0, "arm_deadline_exceeded")
    return value


def wait_terminal(
    client: StdioClient,
    process: subprocess.Popen[bytes],
    run_id: str,
    deadline: float,
    suffix: str,
) -> dict[str, Any]:
    poll = 0
    while True:
        result = client.call(
            query_envelope("get", run_id, f"m7e-get-{suffix}-{poll}"),
            remaining(deadline),
        )
        require(
            result.get("kind") == "run" and isinstance(result.get("run"), dict),
            "run_view_missing",
        )
        run = result["run"]
        if run.get("terminal") is not None:
            return run
        require(process.poll() is None, "app_server_exited")
        time.sleep(0.2)
        poll += 1


def fetch_events(
    client: StdioClient,
    run_id: str,
    deadline: float,
    suffix: str,
) -> list[dict[str, Any]]:
    result = client.call(
        query_envelope("events", run_id, f"m7e-events-{suffix}"),
        remaining(deadline),
    )
    events = result.get("events")
    require(
        result.get("kind") == "events"
        and result.get("run_id") == run_id
        and isinstance(events, list)
        and bool(events),
        "event_ledger_missing",
    )
    require(
        [stored.get("sequence") for stored in events]
        == list(range(1, len(events) + 1)),
        "event_sequence_invalid",
    )
    require(
        all(
            isinstance(stored, dict)
            and stored.get("schema_version") == EVENT_API
            and stored.get("run_id") == run_id
            and isinstance(stored.get("event"), dict)
            for stored in events
        ),
        "event_envelope_invalid",
    )
    return events


def event_kind(stored: dict[str, Any]) -> str:
    event = stored.get("event")
    return event.get("kind", "") if isinstance(event, dict) else ""


def event_values(events: list[dict[str, Any]], kind: str) -> list[dict[str, Any]]:
    return [
        stored["event"]
        for stored in events
        if event_kind(stored) == kind
    ]


def terminal_projection(run: dict[str, Any]) -> dict[str, Any]:
    terminal = run.get("terminal")
    require(isinstance(terminal, dict), "terminal_missing")
    state = terminal.get("state")
    failure = terminal.get("failure")
    return {
        "state": state,
        "failure_sha256": canonical_hash(failure) if failure is not None else None,
    }


def accounting_projection(run: dict[str, Any], expected_effort: str) -> dict[str, Any]:
    accounting = run.get("accounting")
    usage = run.get("usage")
    require(isinstance(accounting, dict) and isinstance(usage, dict), "accounting_missing")
    root = accounting.get("root", {})
    child = accounting.get("child", {})
    surface_usage = accounting.get("surface_usage")
    require(
        isinstance(root, dict)
        and isinstance(child, dict)
        and isinstance(surface_usage, list),
        "accounting_shape_invalid",
    )
    started = root.get("started", 0) + child.get("started", 0)
    completed = root.get("completed", 0) + child.get("completed", 0)
    in_flight = root.get("in_flight", 0) + child.get("in_flight", 0)
    surface_usage_sum = {
        name: sum(
            bucket.get("usage", {}).get(name, 0)
            for bucket in surface_usage
            if isinstance(bucket, dict)
        )
        for name in (
            "input_tokens",
            "output_tokens",
            "cache_hit_tokens",
            "cache_miss_tokens",
            "cache_write_tokens",
            "reasoning_tokens",
            "reasoning_replay_tokens",
        )
    }
    surface_cost = sum(
        bucket.get("cost_nanousd", 0)
        for bucket in surface_usage
        if isinstance(bucket, dict)
    )
    surface_identity_valid = bool(surface_usage) and all(
        isinstance(bucket, dict)
        and bucket.get("surface") == "standard_chat"
        and bucket.get("model") == "deepseek-v4-flash"
        for bucket in surface_usage
    )
    totals_valid = surface_usage_sum == usage and surface_cost == accounting.get("cost_nanousd")
    complete = bool(
        accounting.get("hard_request_limit") == 10
        and accounting.get("sealed") is True
        and accounting.get("complete") is True
        and accounting.get("usage_complete") is True
        and accounting.get("usage_missing") is False
        and accounting.get("usage_incomplete") is False
        and accounting.get("billing_unknown") is False
        and accounting.get("unpriced") is False
        and accounting.get("usage_missing_responses") == 0
        and accounting.get("incomplete_responses") == 0
        and accounting.get("billing_unknown_attempts") == 0
        and accounting.get("unpriced_usage_responses") == 0
        and accounting.get("records_after_seal") == 0
        and accounting.get("transport_retries") == 0
        and accounting.get("runtime_retries") == 0
        and started == completed
        and in_flight == 0
        and surface_identity_valid
        and totals_valid
    )
    off_zero = expected_effort != "off" or (
        usage.get("reasoning_tokens") == 0
        and usage.get("reasoning_replay_tokens") == 0
    )
    return {
        "valid": complete and off_zero,
        "sealed": accounting.get("sealed"),
        "complete": accounting.get("complete"),
        "usage_complete": accounting.get("usage_complete"),
        "billing_unknown": accounting.get("billing_unknown"),
        "unpriced": accounting.get("unpriced"),
        "hard_request_limit": accounting.get("hard_request_limit"),
        "requests": {
            "started": started,
            "completed": completed,
            "in_flight": in_flight,
            "root_started": root.get("started"),
            "child_started": child.get("started"),
        },
        "transport_retries": accounting.get("transport_retries"),
        "runtime_retries": accounting.get("runtime_retries"),
        "usage": usage,
        "surface_usage": surface_usage,
        "surface_identity_valid": surface_identity_valid,
        "surface_totals_valid": totals_valid,
        "off_reasoning_zero": off_zero,
        "cost_nanousd": accounting.get("cost_nanousd"),
    }


def request_projection(
    root_events: list[dict[str, Any]],
    child_events: list[list[dict[str, Any]]],
    expected_effort: str,
) -> dict[str, Any]:
    requests = event_values(root_events, "model_request_prepared")
    for events in child_events:
        requests.extend(event_values(events, "model_request_prepared"))
    efforts = [event.get("request", {}).get("reasoning_effort") for event in requests]
    models = [event.get("request", {}).get("model") for event in requests]
    fingerprints = [
        {
            "actor": event.get("request", {}).get("actor"),
            "system_prompt_sha256": canonical_hash(
                event.get("request", {}).get("system_prompt")
            ),
            "messages_sha256": canonical_hash(event.get("request", {}).get("messages")),
            "tools_sha256": canonical_hash(event.get("request", {}).get("tools")),
            "max_output_tokens": event.get("request", {}).get("max_output_tokens"),
            "streaming": event.get("request", {}).get("streaming"),
        }
        for event in requests
    ]
    return {
        "count": len(requests),
        "efforts": efforts,
        "models": models,
        "valid": bool(requests)
        and set(efforts) == {expected_effort}
        and set(models) == {"deepseek-v4-flash"},
        "fingerprints": fingerprints,
    }


def tool_projection(events_by_run: list[list[dict[str, Any]]]) -> dict[str, Any]:
    prepared: list[dict[str, Any]] = []
    outcomes: list[dict[str, Any]] = []
    for events in events_by_run:
        for event in event_values(events, "tool_prepared"):
            invocation = event.get("invocation", {})
            prepared.append(
                {
                    "name": invocation.get("name"),
                    "workspace_access": event.get("workspace_access"),
                }
            )
        for event in event_values(events, "tool_outcome_committed"):
            outcome = event.get("outcome", {})
            outcomes.append(
                {
                    "name": event.get("name"),
                    "invocation": outcome.get("invocation"),
                    "transport": outcome.get("transport"),
                    "operation": outcome.get("operation"),
                    "side_effect": outcome.get("side_effect"),
                    "retry": outcome.get("retry"),
                    "failure_code": outcome.get("failure_code"),
                }
            )
    return {
        "count": len(prepared),
        "prepared": prepared,
        "outcomes": outcomes,
        "failure_codes": dict(
            sorted(
                Counter(
                    outcome["failure_code"]
                    for outcome in outcomes
                    if outcome["failure_code"]
                ).items()
            )
        ),
    }


def changed_files(workspace: Path) -> list[str]:
    output = git_output(
        "status",
        "--porcelain=v1",
        "--untracked-files=all",
        cwd=workspace,
    )
    values: list[str] = []
    for line in output.splitlines():
        require(len(line) >= 4, "git_status_invalid")
        path = line[3:]
        if " -> " in path:
            path = path.split(" -> ", 1)[1]
        values.append(path)
    return sorted(values)


def external_verifier(workspace: Path, deadline: float) -> dict[str, Any]:
    before = snapshot_tree(workspace)
    started = time.monotonic()
    result = run_command(
        ["/usr/bin/python3", "-I", "-B", "_eval_verifier.py", "."],
        cwd=workspace,
        environment={**safe_env(), "PYTHONDONTWRITEBYTECODE": "1"},
        timeout=max(1, int(min(remaining(deadline), 30))),
    )
    after = snapshot_tree(workspace)
    return {
        "passed": result.returncode == 0,
        "returncode": result.returncode,
        "workspace_unchanged": before == after,
        "stdout_sha256": sha256_bytes(result.stdout),
        "stderr_sha256": sha256_bytes(result.stderr),
        "duration_ms": int((time.monotonic() - started) * 1000),
    }


def state_schema(codewhale_home: Path) -> dict[str, Any]:
    database = codewhale_home / "state.db"
    require(database.is_file(), "state_database_missing")
    connection = sqlite3.connect(f"file:{database}?mode=ro", uri=True)
    try:
        row = connection.execute("PRAGMA user_version").fetchone()
    finally:
        connection.close()
    version = row[0] if row else None
    return {"valid": version == STATE_SCHEMA, "version": version}


def tree_contains(root: Path, needle: bytes) -> bool:
    for path in root.rglob("*"):
        if path.is_file() and not path.is_symlink():
            try:
                if needle in path.read_bytes():
                    return True
            except OSError:
                return True
    return False


def verification_projection(
    task_id: str,
    root_events: list[dict[str, Any]],
) -> dict[str, Any]:
    created = event_values(root_events, "run_created")
    prepared = event_values(root_events, "host_verification_prepared")
    started = event_values(root_events, "host_verification_started")
    committed = event_values(root_events, "host_verification_committed")
    terminals = event_values(root_events, "terminal")
    receipts = [
        event.get("receipt")
        for event in committed
        if isinstance(event.get("receipt"), dict)
    ]
    successful_edits: list[int] = []
    failed_verifiers: list[int] = []
    for stored in root_events:
        if event_kind(stored) != "tool_outcome_committed":
            continue
        event = stored["event"]
        outcome = event.get("outcome", {})
        success = (
            outcome.get("invocation") == "accepted"
            and outcome.get("transport") == "succeeded"
            and outcome.get("operation") == "succeeded"
        )
        if event.get("name") in {"apply_patch", "edit_file"} and success:
            successful_edits.append(stored["sequence"])
        if event.get("name") == "run_verifiers" and not success:
            failed_verifiers.append(stored["sequence"])
    temporal_valid = task_id != "t3" or bool(
        failed_verifiers
        and successful_edits
        and min(failed_verifiers) < min(successful_edits)
    )
    latest = committed[-1] if committed else {}
    latest_outcome = latest.get("outcome", {})
    receipt = receipts[0] if len(receipts) == 1 else {}
    terminal = (
        terminals[0].get("outcome", {}).get("terminal", {})
        if len(terminals) == 1
        else {}
    )
    decision = terminal.get("decision", {}) if terminal.get("state") == "completed" else {}
    satisfied = decision.get("satisfied", [])
    receipt_identity_valid = bool(
        receipt
        and latest.get("workspace_state_after") == receipt.get("workspace_state")
        and decision.get("workspace_state") == receipt.get("workspace_state")
        and decision.get("generation_id") == receipt.get("generation_id")
        and isinstance(satisfied, list)
        and any(
            item.get("kind") == "evidence"
            and item.get("acceptance_id") == receipt.get("acceptance_id")
            and item.get("receipt_id") == receipt.get("id")
            for item in satisfied
            if isinstance(item, dict)
        )
    )
    latest_success = (
        latest_outcome.get("invocation") == "accepted"
        and latest_outcome.get("transport") == "succeeded"
        and latest_outcome.get("operation") == "succeeded"
    )
    ledger_valid = bool(
        len(created) == 1
        and prepared
        and len(prepared) == len(started) == len(committed)
        and len(receipts) == 1
        and latest_success
        and temporal_valid
        and len(terminals) == 1
        and receipt_identity_valid
    )
    return {
        "valid": ledger_valid,
        "run_created_count": len(created),
        "prepared_count": len(prepared),
        "started_count": len(started),
        "committed_count": len(committed),
        "receipt_count": len(receipts),
        "latest_success": latest_success,
        "temporal_valid": temporal_valid,
        "receipt_identity_valid": receipt_identity_valid,
        "contract_sha256": canonical_hash(
            created[0].get("request", {}).get("task_contract")
            if len(created) == 1
            else None
        ),
        "receipt_sha256": canonical_hash(receipts[0]) if len(receipts) == 1 else None,
    }


def child_projection(
    client: StdioClient,
    root_events: list[dict[str, Any]],
    deadline: float,
    suffix: str,
) -> tuple[list[dict[str, Any]], list[list[dict[str, Any]]]]:
    starts = event_values(root_events, "child_started")
    tasks = {
        event.get("task", {}).get("task_id"): event.get("task", {})
        for event in event_values(root_events, "agent_task_prepared")
    }
    children: list[dict[str, Any]] = []
    ledgers: list[list[dict[str, Any]]] = []
    for index, start in enumerate(starts):
        child_id = start.get("child_run_id")
        require(isinstance(child_id, str) and child_id, "child_id_missing")
        child_run_result = client.call(
            query_envelope("get", child_id, f"m7e-child-get-{suffix}-{index}"),
            remaining(deadline),
        )
        require(
            child_run_result.get("kind") == "run"
            and isinstance(child_run_result.get("run"), dict),
            "child_run_missing",
        )
        child_run = child_run_result["run"]
        child_events = fetch_events(
            client,
            child_id,
            deadline,
            f"{suffix}-child-{index}",
        )
        ledgers.append(child_events)
        task = tasks.get(start.get("task_id"), {})
        child_tools = [
            event.get("invocation", {}).get("name")
            for event in event_values(child_events, "tool_prepared")
        ]
        children.append(
            {
                "run_id_sha256": sha256_bytes(child_id.encode()),
                "terminal": terminal_projection(child_run),
                "workspace_access": task.get("workspace_access"),
                "allowed_tools": task.get("allowed_tools"),
                "tool_names": child_tools,
                "event_count": len(child_events),
                "event_counts": dict(
                    sorted(Counter(event_kind(event) for event in child_events).items())
                ),
            }
        )
    return children, ledgers


def child_expectation_valid(
    task_id: str,
    task: dict[str, Any],
    children: list[dict[str, Any]],
    readonly_tools: list[str],
) -> bool:
    expectation = task["child_expectation"]
    if expectation in {"forbidden", "zero"}:
        return not children
    if expectation != "exactly_one_read_only" or len(children) != 1:
        return False
    child = children[0]
    return bool(
        child["terminal"]["state"] == "completed"
        and child["workspace_access"] == "read_only"
        and child["allowed_tools"] == readonly_tools
        and all(name in readonly_tools for name in child["tool_names"])
        and task_id == "t5"
    )


def execute_arm(
    manifest: dict[str, Any],
    tasks: dict[str, Any],
    task_id: str,
    variant: str,
    run_index: int,
    binary_source: Path,
    binary_identity: dict[str, Any],
    revision: str,
    key: str,
) -> dict[str, Any]:
    started = time.monotonic()
    resources = manifest["resources"]
    deadline = started + resources["harness_wall_time_seconds"]
    evaluation_id = uuid.uuid4().hex
    with tempfile.TemporaryDirectory(
        prefix=f"codewhale-m7e-{task_id}-{variant}-"
    ) as raw:
        root = Path(raw)
        workspace = root / "workspace"
        base = materialize_fixture(tasks, task_id, workspace)
        state_root = root / "state"
        home = state_root / "home"
        codewhale_home = state_root / "codewhale"
        xdg = state_root / "xdg"
        for directory in (home, codewhale_home, xdg):
            directory.mkdir(parents=True)
        binary = root / "codewhale"
        shutil.copy2(binary_source, binary)
        binary.chmod(0o700)
        require(
            file_hash(binary) == binary_identity["sha256"]
            and binary.stat().st_size == binary_identity["size_bytes"],
            "binary_changed_after_preflight",
        )
        secret = key.encode("utf-8")
        environment = {
            **safe_env(),
            "HOME": str(home),
            "CODEWHALE_HOME": str(codewhale_home),
            "XDG_CONFIG_HOME": str(xdg),
            "DEEPSEEK_API_KEY": key,
        }
        stderr_path = state_root / "app-server.stderr"
        with stderr_path.open("wb") as stderr_stream:
            process = subprocess.Popen(
                [
                    str(binary),
                    "--provider",
                    "deepseek",
                    "app-server",
                    "--stdio",
                    "--transport-max-retries",
                    str(resources["transport_max_retries_per_request"]),
                ],
                cwd=workspace,
                env=environment,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=stderr_stream,
                start_new_session=True,
            )
        environment["DEEPSEEK_API_KEY"] = ""
        client: StdioClient | None = None
        result: dict[str, Any] = {}
        try:
            client = StdioClient(process, secret)
            suffix = f"{task_id}-{variant}-{run_index}-{evaluation_id}"
            response = client.call(
                start_envelope(
                    manifest,
                    tasks,
                    task_id,
                    variant,
                    workspace,
                    f"m7e-start-{suffix}",
                ),
                remaining(deadline),
            )
            require(
                response.get("kind") == "run"
                and isinstance(response.get("run"), dict),
                "start_run_missing",
            )
            root_id = response["run"].get("run_id")
            require(isinstance(root_id, str) and root_id, "root_id_missing")
            run = wait_terminal(client, process, root_id, deadline, suffix)
            root_events = fetch_events(client, root_id, deadline, suffix)
            children, child_ledgers = child_projection(
                client,
                root_events,
                deadline,
                suffix,
            )
            external = external_verifier(workspace, deadline)
            changed = changed_files(workspace)
            expected_changed = sorted(tasks["tasks"][task_id]["expected_changed_files"])
            allowed_paths = tasks["tasks"][task_id]["allowed_paths"]
            child_valid = child_expectation_valid(
                task_id,
                tasks["tasks"][task_id],
                children,
                tasks["tool_policy"]["readonly_child_tools"],
            )
            verification = verification_projection(task_id, root_events)
            requests = request_projection(
                root_events,
                child_ledgers,
                manifest["experiment"]["variants"][variant],
            )
            accounting = accounting_projection(
                run,
                manifest["experiment"]["variants"][variant],
            )
            tools = tool_projection([root_events, *child_ledgers])
            root_tool_names = [
                event.get("invocation", {}).get("name")
                for event in event_values(root_events, "tool_prepared")
            ]
            tool_authority = all(
                name in tasks["tool_policy"]["root_tools"] for name in root_tool_names
            ) and all(
                all(
                    name in tasks["tool_policy"]["readonly_child_tools"]
                    for name in child["tool_names"]
                )
                for child in children
            )
            behavioral_verified = bool(
                terminal_projection(run)["state"] == "completed"
                and verification["valid"]
                and external["passed"]
                and external["workspace_unchanged"]
                and changed == expected_changed
                and all(path in allowed_paths for path in changed)
                and child_valid
                and tool_authority
            )
            measurement_valid = bool(
                accounting["valid"]
                and requests["valid"]
                and len(root_events) > 0
            )
            result = {
                "task_id": task_id,
                "variant": variant,
                "run_index": run_index,
                "evaluation_id": evaluation_id,
                "revision": revision,
                "binary_sha256": file_hash(binary),
                "fixture_base_commit": base,
                "fixture_tree_sha256": fixture_hash(tasks, task_id),
                "terminal": terminal_projection(run),
                "verified_success": behavioral_verified and measurement_valid,
                "behavioral_verified": behavioral_verified,
                "measurement_valid": measurement_valid,
                "false_success": terminal_projection(run)["state"] == "completed"
                and not behavioral_verified,
                "accounting": accounting,
                "request_identity": requests,
                "verification": verification,
                "tool": tools,
                "child": {
                    "valid": child_valid,
                    "count": len(children),
                    "children": children,
                },
                "external_verifier": external,
                "changed_files": changed,
                "scope_valid": changed == expected_changed
                and all(path in allowed_paths for path in changed),
                "tool_authority_valid": tool_authority,
                "state_schema": None,
                "event_counts": dict(
                    sorted(Counter(event_kind(event) for event in root_events).items())
                ),
                "tree_event_count": len(root_events)
                + sum(len(events) for events in child_ledgers),
                "git_diff_sha256": sha256_bytes(
                    run_command(
                        ["git", "diff", "--binary", "HEAD"],
                        cwd=workspace,
                    ).stdout
                ),
                "final_tree_sha256": canonical_hash(snapshot_tree(workspace)),
                "wall_time_ms": int((time.monotonic() - started) * 1000),
            }
        finally:
            if client is not None:
                client.close()
            stop_process(process)
        result["state_schema"] = state_schema(codewhale_home)
        result["measurement_valid"] = bool(
            result["measurement_valid"] and result["state_schema"]["valid"]
        )
        result["verified_success"] = bool(
            result["behavioral_verified"] and result["measurement_valid"]
        )
        stderr = stderr_path.read_bytes()
        require(secret not in stderr, "credential_in_stderr")
        require(not tree_contains(workspace, secret), "credential_in_workspace")
        require(not tree_contains(state_root, secret), "credential_in_state")
        require(secret not in canonical_bytes(result), "credential_in_result")
        return result


def probe_binary(binary: Path, revision: str) -> dict[str, Any]:
    require(binary.is_file() and not binary.is_symlink(), "binary_invalid")
    result = run_command([str(binary), "--version"], cwd=ROOT, timeout=30)
    require(result.returncode == 0 and not result.stderr, "binary_version_failed")
    version = result.stdout.decode("utf-8", errors="strict").strip()
    require(revision[:12] in version, "binary_revision_mismatch")
    return {
        "revision": revision,
        "version": version,
        "sha256": file_hash(binary),
        "size_bytes": binary.stat().st_size,
    }


def preflight_identity(
    manifest: dict[str, Any],
    tasks: dict[str, Any],
    binary: Path,
    revision: str,
) -> dict[str, Any]:
    require(len(revision) == 40, "revision_invalid")
    require(git_output("rev-parse", "HEAD") == revision, "revision_head_mismatch")
    require(git_output("status", "--short") == "", "worktree_not_clean")
    for task_id in manifest["task_source"]["task_ids"]:
        require(
            fixture_hash(tasks, task_id)
            == tasks["tasks"][task_id]["fixture_tree_sha256"],
            "fixture_hash_mismatch",
            {"task_id": task_id},
        )
    identity = probe_binary(binary, revision)
    return {
        "binary": identity,
        "source_tree": git_output("rev-parse", f"{revision}^{{tree}}"),
        "manifest_sha256": file_hash(MANIFEST_PATH),
        "manifest_content_sha256": manifest_content_hash(manifest),
        "harness_sha256": file_hash(Path(__file__).resolve()),
        "harness_test_sha256": file_hash(TEST_PATH),
        "task_source_sha256": file_hash(TASK_SOURCE_PATH),
        "schedule_sha256": canonical_hash(formal_schedule(manifest)),
        "schedule_arms": len(formal_schedule(manifest)),
    }


def accounting_abort_code(
    manifest: dict[str, Any],
    arm: dict[str, Any],
) -> str | None:
    accounting = arm.get("accounting", {})
    resources = manifest["resources"]
    if accounting.get("billing_unknown") is not False:
        return "aborted_unknown_billing"
    if accounting.get("unpriced") is not False:
        return "aborted_unpriced"
    if accounting.get("sealed") is not True:
        return "aborted_unsealed"
    if not accounting.get("valid"):
        return "aborted_accounting_incomplete"
    if not arm.get("measurement_valid"):
        return "aborted_measurement_invalid"
    if (
        accounting.get("cost_nanousd", 0)
        > resources["max_known_cost_nanousd_per_arm"]
    ):
        return "aborted_per_arm_known_cost_threshold"
    if (
        accounting.get("requests", {}).get("started", 0)
        > resources["max_physical_api_attempts_per_arm"]
    ):
        return "aborted_physical_request_limit"
    for outcome in arm.get("tool", {}).get("outcomes", []):
        succeeded = (
            outcome.get("invocation") == "accepted"
            and outcome.get("transport") == "succeeded"
            and outcome.get("operation") == "succeeded"
            and outcome.get("retry") == "not_needed"
            and outcome.get("failure_code") is None
        )
        if not succeeded and (
            outcome.get("operation") == "indeterminate"
            or outcome.get("side_effect") == "indeterminate"
            or outcome.get("failure_code") == "side_effect_ambiguous"
        ):
            return "aborted_side_effect_ambiguous"
    return None


def median_fraction(values: list[Fraction]) -> Fraction | None:
    if not values:
        return None
    ordered = sorted(values)
    middle = len(ordered) // 2
    if len(ordered) % 2:
        return ordered[middle]
    return (ordered[middle - 1] + ordered[middle]) / 2


def arm_metric(arm: dict[str, Any], name: str) -> int:
    if name == "physical_requests":
        return int(arm["accounting"]["requests"]["started"])
    if name == "tokens":
        usage = arm["accounting"]["usage"]
        return int(usage["input_tokens"] + usage["output_tokens"])
    if name == "cost_nanousd":
        return int(arm["accounting"]["cost_nanousd"])
    if name == "wall_time_ms":
        return int(arm["wall_time_ms"])
    raise AssertionError(name)


def paired_summary(manifest: dict[str, Any], arms: list[dict[str, Any]]) -> dict[str, Any]:
    by_pair: dict[tuple[str, int], dict[str, dict[str, Any]]] = defaultdict(dict)
    for arm in arms:
        by_pair[(arm["task_id"], arm["run_index"])][arm["variant"]] = arm
    complete_pairs = [
        pair for pair in by_pair.values() if set(pair) == set(VARIANTS)
    ]
    dual_success = [
        pair
        for pair in complete_pairs
        if all(pair[variant]["verified_success"] for variant in VARIANTS)
    ]
    metrics: dict[str, Any] = {}
    for name in ("physical_requests", "tokens", "cost_nanousd", "wall_time_ms"):
        improvements: list[Fraction] = []
        for pair in dual_success:
            before = arm_metric(pair["reasoning_high"], name)
            after = arm_metric(pair["reasoning_off"], name)
            if before > 0:
                improvements.append(Fraction(before - after, before))
        median = median_fraction(improvements)
        metrics[name] = {
            "pairs": len(improvements),
            "paired_median_improvement_percent": (
                float(median * 100) if median is not None else None
            ),
        }
    return {
        "complete_pairs": len(complete_pairs),
        "dual_success_pairs": len(dual_success),
        "metrics": metrics,
    }


def decide(manifest: dict[str, Any], arms: list[dict[str, Any]]) -> dict[str, Any]:
    expected = manifest["experiment"]["formal_arms"]
    if len(arms) != expected:
        return {"decision": "hold", "reason": "formal_cells_incomplete"}
    cells = Counter((arm["task_id"], arm["variant"]) for arm in arms)
    if set(cells.values()) != {manifest["experiment"]["runs_per_variant_task"]}:
        return {"decision": "hold", "reason": "formal_cell_shape_invalid"}
    if any(arm["false_success"] for arm in arms):
        return {"decision": "reject", "reason": "false_success"}
    if any(
        not arm.get("scope_valid")
        or not arm.get("tool_authority_valid")
        or not arm.get("child", {}).get("valid")
        or arm.get("tool", {}).get("count", 0)
        > manifest["resources"]["max_tool_calls_per_arm"]
        or arm.get("request_identity", {}).get("count", 0)
        > manifest["resources"]["max_logical_model_requests_per_arm"]
        or arm.get("wall_time_ms", 0)
        > manifest["resources"]["harness_wall_time_seconds"] * 1000
        for arm in arms
    ):
        return {"decision": "reject", "reason": "authority_or_budget_gate_failed"}
    if any(not arm["measurement_valid"] for arm in arms):
        return {"decision": "hold", "reason": "measurement_incomplete"}
    by_pair: dict[tuple[str, int], dict[str, dict[str, Any]]] = defaultdict(dict)
    for arm in arms:
        by_pair[(arm["task_id"], arm["run_index"])][arm["variant"]] = arm
    for pair in by_pair.values():
        if set(pair) != set(VARIANTS):
            return {"decision": "hold", "reason": "paired_identity_incomplete"}
        high = pair["reasoning_high"]
        off = pair["reasoning_off"]
        if (
            high["revision"] != off["revision"]
            or high["binary_sha256"] != off["binary_sha256"]
            or high["fixture_tree_sha256"] != off["fixture_tree_sha256"]
            or high["request_identity"]["fingerprints"][0]
            != off["request_identity"]["fingerprints"][0]
        ):
            return {"decision": "reject", "reason": "paired_treatment_identity_mismatch"}
    task_ids = manifest["task_source"]["task_ids"]
    for task_id in task_ids:
        high = sum(
            arm["verified_success"]
            for arm in arms
            if arm["task_id"] == task_id and arm["variant"] == "reasoning_high"
        )
        off = sum(
            arm["verified_success"]
            for arm in arms
            if arm["task_id"] == task_id and arm["variant"] == "reasoning_off"
        )
        if off < high:
            return {
                "decision": "reject",
                "reason": "verified_success_regression",
                "task_id": task_id,
            }
    paired = paired_summary(manifest, arms)
    if paired["dual_success_pairs"] < 12:
        return {
            "decision": "hold",
            "reason": "insufficient_dual_success_pairs",
            "paired": paired,
        }
    improvements = [
        value["paired_median_improvement_percent"]
        for value in paired["metrics"].values()
        if value["paired_median_improvement_percent"] is not None
    ]
    benefit = any(value >= 15 for value in improvements)
    no_hidden_regression = all(value >= -10 for value in improvements)
    return {
        "decision": "keep" if benefit and no_hidden_regression else "hold",
        "reason": (
            "benefit_threshold_met"
            if benefit and no_hidden_regression
            else "net_benefit_not_proven"
        ),
        "paired": paired,
    }


def aggregate(manifest: dict[str, Any], arms: list[dict[str, Any]]) -> dict[str, Any]:
    cells: dict[str, Any] = {}
    for task_id in manifest["task_source"]["task_ids"]:
        cells[task_id] = {}
        for variant in VARIANTS:
            selected = [
                arm
                for arm in arms
                if arm["task_id"] == task_id and arm["variant"] == variant
            ]
            cells[task_id][variant] = {
                "runs": len(selected),
                "verified_success": sum(arm["verified_success"] for arm in selected),
                "false_success": sum(arm["false_success"] for arm in selected),
                "physical_requests": sum(
                    arm["accounting"]["requests"]["started"] for arm in selected
                ),
                "input_tokens": sum(
                    arm["accounting"]["usage"]["input_tokens"] for arm in selected
                ),
                "output_tokens": sum(
                    arm["accounting"]["usage"]["output_tokens"] for arm in selected
                ),
                "reasoning_tokens": sum(
                    arm["accounting"]["usage"]["reasoning_tokens"] for arm in selected
                ),
                "reasoning_replay_tokens": sum(
                    arm["accounting"]["usage"]["reasoning_replay_tokens"]
                    for arm in selected
                ),
                "cost_nanousd": sum(
                    arm["accounting"]["cost_nanousd"] for arm in selected
                ),
            }
    return {
        "arms": len(arms),
        "cells": cells,
        "paired": paired_summary(manifest, arms),
        "decision": decide(manifest, arms),
    }


def write_private_json(path: Path, value: dict[str, Any], *, replace: bool) -> None:
    encoded = canonical_bytes(value) + b"\n"
    path.parent.mkdir(parents=True, exist_ok=True)
    if not replace:
        try:
            descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        except FileExistsError as error:
            raise EvaluationError("output_exists") from error
        try:
            offset = 0
            while offset < len(encoded):
                offset += os.write(descriptor, encoded[offset:])
            os.fsync(descriptor)
        finally:
            os.close(descriptor)
        return
    temporary = path.with_name(f".{path.name}.tmp-{uuid.uuid4().hex}")
    try:
        descriptor = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        try:
            offset = 0
            while offset < len(encoded):
                offset += os.write(descriptor, encoded[offset:])
            os.fsync(descriptor)
        finally:
            os.close(descriptor)
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)
    os.chmod(path, 0o600)


def claim_output(path: Path, identity: dict[str, Any]) -> dict[str, Any]:
    require(path.suffix == ".json", "output_suffix_invalid")
    require(
        path.parent.resolve() == (ROOT / "eval/raw").resolve(),
        "output_directory_invalid",
    )
    require(not path.exists(), "output_exists")
    partial = path.with_suffix(path.suffix + ".partial")
    require(not partial.exists(), "partial_output_exists")
    reservation = {
        "schema": RESULT_SCHEMA,
        "status": "reserved_before_key",
        "created_at_unix_ms": int(time.time() * 1000),
        "identity": identity,
        "schedule": formal_schedule(load_manifest(frozen=True)[0]),
        "active_arm": None,
        "arms": [],
        "aggregate": None,
        "abort": None,
        "known_cost_is_lower_bound": True,
    }
    write_private_json(path, reservation, replace=False)
    require(stat.S_IMODE(path.stat().st_mode) == 0o600, "output_mode_invalid")
    return reservation


def read_key(path: Path) -> str:
    require(path.is_file() and not path.is_symlink(), "key_file_invalid")
    require(stat.S_IMODE(path.stat().st_mode) & 0o077 == 0, "key_file_permissions_invalid")
    value = path.read_text(encoding="utf-8").strip()
    require(value and "\n" not in value and "\r" not in value, "key_format_invalid")
    require(len(value) >= 16, "key_format_invalid")
    return value


def run_formal(args: argparse.Namespace) -> int:
    manifest, tasks = load_manifest(frozen=True)
    binary = Path(args.binary).resolve()
    revision = args.revision
    require(args.acknowledge_cost, "cost_not_acknowledged")
    identity = preflight_identity(manifest, tasks, binary, revision)
    output = Path(args.output).resolve()
    reservation = claim_output(output, identity)
    key = ""
    try:
        key = read_key(Path(args.key_file).resolve())
    except EvaluationError as error:
        reservation["status"] = "aborted_before_api"
        reservation["abort"] = {"code": error.code, "details": error.details}
        write_private_json(output, reservation, replace=True)
        return 2
    secret = key.encode("utf-8")
    try:
        require(secret not in canonical_bytes(reservation), "credential_in_reservation")
        with tempfile.TemporaryDirectory(prefix="codewhale-m7e-suite-binary-") as raw:
            suite_binary = Path(raw) / "codewhale"
            shutil.copy2(binary, suite_binary)
            suite_binary.chmod(0o700)
            require(
                file_hash(suite_binary) == identity["binary"]["sha256"],
                "suite_binary_changed",
            )
            for arm_spec in reservation["schedule"]:
                known_cost = sum(
                    arm.get("accounting", {}).get("cost_nanousd", 0)
                    for arm in reservation["arms"]
                )
                if (
                    known_cost
                    + manifest["resources"]["max_known_cost_nanousd_per_arm"]
                    > manifest["resources"]["formal_suite_known_cost_nanousd"]
                ):
                    raise EvaluationError("suite_known_cost_headroom_exhausted")
                reservation["active_arm"] = arm_spec
                reservation["status"] = "running"
                reservation["known_cost_is_lower_bound"] = True
                write_private_json(output, reservation, replace=True)
                arm = execute_arm(
                    manifest,
                    tasks,
                    arm_spec["task_id"],
                    arm_spec["variant"],
                    arm_spec["run_index"],
                    suite_binary,
                    identity["binary"],
                    revision,
                    key,
                )
                require(secret not in canonical_bytes(arm), "credential_in_arm")
                reservation["arms"].append(arm)
                reservation["active_arm"] = None
                abort_code = accounting_abort_code(manifest, arm)
                reservation["known_cost_is_lower_bound"] = abort_code is not None
                reservation["aggregate"] = aggregate(manifest, reservation["arms"])
                if abort_code is not None:
                    raise EvaluationError(abort_code)
                reservation["status"] = "running"
                write_private_json(output, reservation, replace=True)
    except EvaluationError as error:
        reservation["status"] = "aborted"
        reservation["abort"] = {
            "code": error.code,
            "details": error.details,
            "active_arm": reservation.get("active_arm"),
        }
        write_private_json(output, reservation, replace=True)
        return 2
    except (OSError, UnicodeError, subprocess.SubprocessError, sqlite3.Error) as error:
        reservation["status"] = "aborted"
        reservation["abort"] = {
            "code": "harness_internal_error",
            "error_type": type(error).__name__,
            "active_arm": reservation.get("active_arm"),
        }
        write_private_json(output, reservation, replace=True)
        return 2
    reservation["status"] = "complete"
    reservation["known_cost_is_lower_bound"] = False
    reservation["aggregate"] = aggregate(manifest, reservation["arms"])
    write_private_json(output, reservation, replace=True)
    require(secret not in output.read_bytes(), "credential_in_output")
    print(
        json.dumps(
            {
                "status": reservation["status"],
                "output": str(output),
                "decision": reservation["aggregate"]["decision"],
                "arms": len(reservation["arms"]),
            },
            ensure_ascii=False,
            sort_keys=True,
        )
    )
    return 0


def freeze_report() -> dict[str, Any]:
    manifest, tasks = load_manifest(frozen=False)
    return {
        "schema": SCHEMA,
        "manifest_path": MANIFEST_PATH.relative_to(ROOT).as_posix(),
        "manifest_sha256": file_hash(MANIFEST_PATH),
        "manifest_content_sha256_excluding_frozen_hashes": manifest_content_hash(
            manifest
        ),
        "harness_sha256": file_hash(Path(__file__).resolve()),
        "harness_test_sha256": file_hash(TEST_PATH) if TEST_PATH.exists() else None,
        "task_source_sha256": file_hash(TASK_SOURCE_PATH),
        "schedule_sha256": canonical_hash(formal_schedule(manifest)),
        "schedule_arms": len(formal_schedule(manifest)),
        "fixtures": {
            task_id: fixture_hash(tasks, task_id)
            for task_id in manifest["task_source"]["task_ids"]
        },
    }


def preflight_command(args: argparse.Namespace) -> int:
    manifest, tasks = load_manifest(frozen=True)
    identity = preflight_identity(
        manifest,
        tasks,
        Path(args.binary).resolve(),
        args.revision,
    )
    print(json.dumps(identity, ensure_ascii=False, sort_keys=True, indent=2))
    return 0


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)
    subparsers.add_parser("freeze-report")
    preflight = subparsers.add_parser("preflight")
    preflight.add_argument("--binary", required=True)
    preflight.add_argument("--revision", required=True)
    formal = subparsers.add_parser("formal")
    formal.add_argument("--binary", required=True)
    formal.add_argument("--revision", required=True)
    formal.add_argument("--key-file", required=True)
    formal.add_argument("--output", required=True)
    formal.add_argument("--acknowledge-cost", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        if args.command == "freeze-report":
            print(json.dumps(freeze_report(), ensure_ascii=False, sort_keys=True, indent=2))
            return 0
        if args.command == "preflight":
            return preflight_command(args)
        if args.command == "formal":
            return run_formal(args)
        raise AssertionError(args.command)
    except EvaluationError as error:
        print(
            json.dumps(
                {"status": "error", "code": error.code, "details": error.details},
                ensure_ascii=False,
                sort_keys=True,
            ),
            file=sys.stderr,
        )
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
