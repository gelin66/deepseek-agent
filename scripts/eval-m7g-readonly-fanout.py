#!/usr/bin/env python3
"""M7-G same-binary read-only fan-out admission evaluator.

The harness speaks Run API v10 and projects only canonical RuntimeEvent v16,
RunStore, verifier, and accounting facts. It does not emulate tools, classify
model text, or implement a second agent loop.
"""

from __future__ import annotations

import argparse
from collections import Counter
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
from typing import Any, BinaryIO
import uuid


ROOT = Path(__file__).resolve().parents[1]
MANIFEST_PATH = ROOT / "eval/manifests/m7-g-readonly-fanout-v1.json"
FIXTURE_ROOT = ROOT / "eval/fixtures/m7-readonly-fanout"
SCHEMA = "codewhale.eval.m7-g-readonly-fanout-result.v1"
RUN_API = 10
EVENT_API = 16
STATE_SCHEMA = 21
MODEL = "deepseek-v4-flash"
VARIANTS = ("control", "treatment")
RUNS_PER_CELL = 3
MAX_API_REQUESTS = 12
MAX_TURNS = 12
MAX_TOOL_CALLS = 24
MAX_OUTPUT_TOKENS = 4096
WALL_TIME_SECONDS = 300
MODEL_IDLE_MS = 120_000
HARNESS_GRACE_SECONDS = 30
PER_ARM_COST_CEILING_USD = 0.02
MAX_FRAME = 16 * 1024 * 1024
HOST_VERIFIER_TIMEOUT_MS = 600_000
HOST_VERIFIER_ENV = {"PYTHONDONTWRITEBYTECODE": "1"}
ROOT_TOOLS = [
    "apply_patch",
    "edit_file",
    "file_search",
    "grep_files",
    "list_dir",
    "read_file",
    "run_verifiers",
]
CHILD_TOOLS = ["read_file", "list_dir", "grep_files"]
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
NETWORK_OVERRIDE_NAMES = {
    "DEEPSEEK_API_BASE",
    "DEEPSEEK_BASE_URL",
    "OPENAI_API_BASE",
    "OPENAI_BASE_URL",
}
TASKS = {
    "t1-capability-intersection": {
        "target": "compatibility.py",
        "objective": (
            "实现 compatibility.py 中的 compatibility()。读取 README.md 与 specs/ "
            "中的全部 JSON，按仓库契约计算最高 minimum、所有组件共享 feature 的排序交集，"
            "以及排序后的组件名。不得硬编码 fixture 值。"
        ),
        "partitions": [
            "只读检查 parser/runtime 规格并归纳局部 minimum 与 feature 交集",
            "只读检查 state 规格与 README 契约并归纳边界/反例",
        ],
    },
    "t2-dependency-impact": {
        "target": "impact.py",
        "objective": (
            "实现 impact.py 中的 affected_components(changed)。读取 README.md 与 "
            "components/ 中的全部 JSON，返回包含 changed 自身的传递反向依赖排序列表；"
            "未知组件返回空列表。不得硬编码 fixture 值。"
        ),
        "partitions": [
            "只读检查 protocol/runtime/state 依赖半区并归纳局部影响边",
            "只读检查 tools/app/tui 依赖半区与未知组件反例",
        ],
    },
    "t3-policy-resolution": {
        "target": "policy.py",
        "objective": (
            "实现 policy.py 中的 resolve_policy()。读取 README.md 和 policies/ 的 "
            "base/region/tenant JSON，按顺序递归合并，同时禁止后续层覆盖 base.locked_keys "
            "声明的点路径；返回值移除 locked_keys。不得硬编码 fixture 值或修改输入。"
        ),
        "partitions": [
            "只读检查 base/region 的递归合并与优先级语义",
            "只读检查 tenant 覆盖、locked_keys 点路径与不变性反例",
        ],
    },
}


class EvaluationError(RuntimeError):
    def __init__(self, code: str, details: dict[str, Any] | None = None) -> None:
        super().__init__(code)
        self.code = code
        self.details = details or {}


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


def load_manifest(*, frozen: bool) -> dict[str, Any]:
    try:
        manifest = json.loads(MANIFEST_PATH.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise EvaluationError("manifest_unavailable") from error
    require(
        isinstance(manifest, dict)
        and manifest.get("schema") == "codewhale.eval.m7-g-readonly-fanout.v1",
        "manifest_schema_invalid",
    )
    source = manifest.get("source_identity", {})
    require(
        source.get("run_api") == RUN_API
        and source.get("runtime_event") == EVENT_API
        and source.get("state_schema") == STATE_SCHEMA
        and source.get("exec_stream") == 2,
        "protocol_identity_invalid",
    )
    experiment = manifest.get("experiment", {})
    require(
        experiment.get("runs_per_variant_task") == RUNS_PER_CELL
        and experiment.get("formal_pairs") == 9
        and experiment.get("formal_arms") == 18,
        "experiment_identity_invalid",
    )
    require(
        [task.get("id") for task in manifest.get("tasks", [])] == list(TASKS),
        "task_identity_invalid",
    )
    if frozen:
        hashes = manifest.get("frozen_hashes", {})
        without_hashes = dict(manifest)
        without_hashes.pop("frozen_hashes", None)
        require(
            hashes.get("harness_sha256") == file_hash(Path(__file__).resolve())
            and hashes.get("schedule_sha256") == canonical_hash(formal_schedule())
            and hashes.get("task_contracts_sha256")
            == canonical_hash(
                {
                    task_id: task_definition(task_id)
                    for task_id in TASKS
                }
            )
            and hashes.get("manifest_content_sha256_excluding_frozen_hashes")
            == canonical_hash(without_hashes),
            "frozen_hash_mismatch",
        )
    return manifest


def formal_schedule() -> list[dict[str, Any]]:
    task_ids = list(TASKS)
    schedule: list[dict[str, Any]] = []
    for run_index in range(RUNS_PER_CELL):
        rotated = task_ids[run_index:] + task_ids[:run_index]
        for task_index, task_id in enumerate(rotated):
            pair_index = run_index * len(task_ids) + task_index
            order = (
                VARIANTS
                if pair_index % 2 == 0
                else tuple(reversed(VARIANTS))
            )
            for position, variant in enumerate(order):
                schedule.append(
                    {
                        "pair_index": pair_index,
                        "task_id": task_id,
                        "run_index": run_index,
                        "variant": variant,
                        "pair_position": position,
                    }
                )
    return schedule


def safe_env() -> dict[str, str]:
    environment = {
        name: value
        for name in SAFE_ENV_NAMES
        if (value := os.environ.get(name))
    }
    require(
        not any(name in os.environ for name in NETWORK_OVERRIDE_NAMES),
        "network_override_present",
    )
    environment.setdefault("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
    environment["PYTHONDONTWRITEBYTECODE"] = "1"
    environment["GIT_CONFIG_NOSYSTEM"] = "1"
    environment["GIT_TERMINAL_PROMPT"] = "0"
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
            timeout=timeout,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise EvaluationError("command_failed") from error


def git_output(*arguments: str, cwd: Path = ROOT) -> str:
    result = run_command(["git", *arguments], cwd=cwd)
    require(result.returncode == 0, "git_failed")
    try:
        return result.stdout.decode("utf-8").strip()
    except UnicodeDecodeError as error:
        raise EvaluationError("git_output_invalid") from error


def fixture_hash(path: Path) -> str:
    records: list[dict[str, Any]] = []
    for item in sorted(path.rglob("*")):
        if item.is_file() and not item.is_symlink():
            records.append(
                {
                    "path": item.relative_to(path).as_posix(),
                    "mode": stat.S_IMODE(item.stat().st_mode),
                    "sha256": file_hash(item),
                }
            )
    return canonical_hash(records)


def materialize_fixture(task_id: str, destination: Path) -> str:
    source = FIXTURE_ROOT / task_id
    require(source.is_dir() and not source.is_symlink(), "fixture_unavailable")
    shutil.copytree(source, destination)
    before = fixture_hash(destination)
    environment = {
        **safe_env(),
        "GIT_AUTHOR_NAME": "CodeWhale M7-G",
        "GIT_AUTHOR_EMAIL": "m7g@example.invalid",
        "GIT_COMMITTER_NAME": "CodeWhale M7-G",
        "GIT_COMMITTER_EMAIL": "m7g@example.invalid",
        "GIT_AUTHOR_DATE": "2026-07-23T00:00:00Z",
        "GIT_COMMITTER_DATE": "2026-07-23T00:00:00Z",
    }
    for arguments in (
        ["git", "init", "-q", "-b", "main"],
        ["git", "add", "."],
        ["git", "-c", "core.hooksPath=/dev/null", "commit", "-q", "-m", "fixture"],
    ):
        result = run_command(arguments, cwd=destination, environment=environment)
        require(result.returncode == 0, "fixture_git_failed")
    require(not git_output("status", "--porcelain=v1", cwd=destination), "fixture_dirty")
    return before


def verifier_spec(task_id: str) -> dict[str, Any]:
    program = str(Path(sys.executable).resolve())
    command = {
        "name": f"{task_id}-exact",
        "program": program,
        "args": ["-I", "-B", "_eval_verifier.py"],
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
                    "id": command["name"],
                    "program": program,
                    "args": command["args"],
                    "cwd": "",
                    "env": HOST_VERIFIER_ENV,
                    "timeout_ms": HOST_VERIFIER_TIMEOUT_MS,
                }
            ]
        },
    }


def task_prompt(task_id: str) -> str:
    task = TASKS[task_id]
    first, second = task["partitions"]
    return f"""任务：{task["objective"]}

共同约束：
1. 只允许修改 `{task["target"]}`；不得修改 README、JSON、verifier、Git 元数据或新增文件。
2. 不得读取环境变量、凭据或仓库外路径。
3. 完成必须以最新 workspace revision 的 deterministic verifier 为准，不得自称测试通过。

条件执行协议（control/treatment 的文本逐字相同）：
- 如果当前工具目录没有 `agent`，由根 Agent 自己只读调查全部文件，然后修改目标文件。
- 如果当前工具目录提供 `agent`，第一次模型回合必须在同一个 assistant response 中恰好调用两次
  `agent`，两者都使用 `type="explore"`、`fork_context=false`、`max_steps=3`、
  `wall_time_secs=120`、`allowed_tools={json.dumps(CHILD_TOOLS)}`，且只读分工分别为：
  A. {first}；
  B. {second}。
  发出这两个调用的同一回合不得调用其他工具。收到两个 typed handoff 后，只有根 Agent
  可以修改 `{task["target"]}`；不得启动第三个子 Agent。
- 修改后可调用 `run_verifiers` 做 exact 验证；最终完成仍由 Host 接受。
"""


def task_definition(task_id: str) -> dict[str, Any]:
    task = TASKS[task_id]
    return {
        "objective": task_prompt(task_id),
        "constraints": [
            f"只修改 {task['target']}",
            "agent 可用时同一回合恰好启动两个只读 Explorer",
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
                "verifier": verifier_spec(task_id),
            }
        ],
    }


def start_envelope(
    task_id: str, variant: str, workspace: Path, request_id: str
) -> dict[str, Any]:
    allowed = list(ROOT_TOOLS)
    if variant == "treatment":
        allowed.append("agent")
    return {
        "schema_version": RUN_API,
        "request_id": request_id,
        "command": {
            "kind": "start",
            "task": task_definition(task_id),
            "workspace": str(workspace.resolve()),
            "model": MODEL,
            "reasoning_effort": "high",
            "max_output_tokens": MAX_OUTPUT_TOKENS,
            "max_api_requests": MAX_API_REQUESTS,
            "streaming": True,
            "tool_policy": {"enabled": True, "allowed": allowed, "denied": []},
            "limits": {
                "max_turns": MAX_TURNS,
                "max_model_requests": MAX_API_REQUESTS,
                "max_model_retries": 0,
                "max_tool_calls": MAX_TOOL_CALLS,
                "max_depth": 0 if variant == "control" else 1,
                "max_concurrent_children": 0 if variant == "control" else 2,
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
        self.buffer = bytearray()
        os.set_blocking(self.stdin_fd, False)
        os.set_blocking(self.stdout_fd, False)

    def call(self, envelope: dict[str, Any], timeout: float = 30.0) -> dict[str, Any]:
        encoded = canonical_bytes(envelope) + b"\n"
        require(self.forbidden not in encoded, "key_in_protocol")
        deadline = time.monotonic() + timeout
        view = memoryview(encoded)
        while view:
            try:
                written = os.write(self.stdin_fd, view)
            except BlockingIOError:
                written = 0
            except OSError as error:
                raise EvaluationError("stdio_write_failed") from error
            if written:
                view = view[written:]
                continue
            self._wait(self.stdin_fd, selectors.EVENT_WRITE, deadline)
        line = self._readline(deadline)
        require(self.forbidden not in line, "key_in_protocol")
        try:
            response = json.loads(line)
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise EvaluationError("stdio_response_invalid") from error
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
            raise EvaluationError(
                "run_api_error",
                {
                    "code": error.get("code"),
                    "reason": error.get("reason"),
                },
            )
        return result

    def _wait(self, descriptor: int, event: int, deadline: float) -> None:
        remaining = deadline - time.monotonic()
        require(remaining > 0, "stdio_timeout")
        with selectors.DefaultSelector() as selector:
            selector.register(descriptor, event)
            require(bool(selector.select(remaining)), "stdio_timeout")

    def _readline(self, deadline: float) -> bytes:
        while True:
            if newline := self.buffer.find(b"\n") + 1:
                line = bytes(self.buffer[:newline])
                del self.buffer[:newline]
                return line
            require(len(self.buffer) <= MAX_FRAME, "stdio_frame_too_large")
            try:
                chunk = os.read(self.stdout_fd, 65_536)
            except BlockingIOError:
                chunk = b""
            except OSError as error:
                raise EvaluationError("stdio_read_failed") from error
            if chunk:
                self.buffer.extend(chunk)
                continue
            require(self.process.poll() is None, "app_server_exited")
            self._wait(self.stdout_fd, selectors.EVENT_READ, deadline)


def stop_process(process: subprocess.Popen[bytes]) -> None:
    if process.poll() is not None:
        return
    try:
        os.killpg(process.pid, signal.SIGTERM)
        process.wait(timeout=5)
    except (OSError, subprocess.TimeoutExpired):
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except OSError:
            pass
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            pass


def event_kind(stored: dict[str, Any]) -> str:
    event = stored.get("event", {})
    return event.get("kind") if isinstance(event, dict) else ""


def fetch_events(client: StdioClient, run_id: str, suffix: str) -> list[dict[str, Any]]:
    result = client.call(query_envelope("events", run_id, f"events-{suffix}"))
    require(result.get("kind") == "events", "events_missing")
    events = result.get("events")
    require(
        isinstance(events, list)
        and all(isinstance(event, dict) for event in events),
        "events_invalid",
    )
    sequences = [event.get("sequence") for event in events]
    require(sequences == list(range(1, len(events) + 1)), "event_sequence_invalid")
    require(
        all(event.get("schema_version") == EVENT_API for event in events),
        "event_schema_invalid",
    )
    return events


def wait_terminal(
    client: StdioClient, run: dict[str, Any], deadline: float, suffix: str
) -> dict[str, Any]:
    run_id = run.get("run_id")
    require(isinstance(run_id, str) and run_id, "run_id_missing")
    poll = 0
    while run.get("terminal") is None:
        require(time.monotonic() < deadline, "run_deadline")
        poll += 1
        time.sleep(0.2)
        result = client.call(
            query_envelope("get", run_id, f"get-{suffix}-{poll}"),
            min(30.0, max(1.0, deadline - time.monotonic())),
        )
        require(result.get("kind") == "run", "run_view_missing")
        run = result.get("run")
        require(isinstance(run, dict), "run_view_missing")
    return run


def accounting_projection(run: dict[str, Any]) -> dict[str, Any]:
    accounting = run.get("accounting", {})
    root = accounting.get("root", {})
    child = accounting.get("child", {})
    usage = run.get("usage", {})
    require(
        isinstance(accounting, dict)
        and isinstance(root, dict)
        and isinstance(child, dict)
        and accounting.get("hard_request_limit") == MAX_API_REQUESTS,
        "accounting_identity_invalid",
    )
    safety_fields = {
        "complete": accounting.get("complete"),
        "usage_complete": accounting.get("usage_complete"),
        "usage_missing": accounting.get("usage_missing"),
        "usage_incomplete": accounting.get("usage_incomplete"),
        "billing_unknown": accounting.get("billing_unknown"),
        "unpriced": accounting.get("unpriced"),
    }
    require(
        safety_fields
        == {
            "complete": True,
            "usage_complete": True,
            "usage_missing": False,
            "usage_incomplete": False,
            "billing_unknown": False,
            "unpriced": False,
        },
        "accounting_incomplete",
        safety_fields,
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
    require(
        all(
            item.get("surface") == "standard_chat"
            and item.get("model") == MODEL
            for item in surfaces
            if isinstance(item, dict)
        ),
        "surface_identity_invalid",
    )
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


def changed_files(workspace: Path) -> list[str]:
    result = run_command(
        ["git", "status", "--porcelain=v1", "-z", "--untracked-files=all"],
        cwd=workspace,
    )
    require(result.returncode == 0, "git_status_failed")
    changed = []
    for record in result.stdout.split(b"\0"):
        if not record:
            continue
        require(len(record) >= 4, "git_status_invalid")
        try:
            changed.append(record[3:].decode("utf-8"))
        except UnicodeDecodeError as error:
            raise EvaluationError("git_status_invalid") from error
    return sorted(changed)


def external_verifier(workspace: Path) -> dict[str, Any]:
    started = time.monotonic()
    result = run_command(
        [str(Path(sys.executable).resolve()), "-I", "-B", "_eval_verifier.py"],
        cwd=workspace,
        timeout=120,
    )
    return {
        "passed": result.returncode == 0,
        "returncode": result.returncode,
        "duration_ms": int((time.monotonic() - started) * 1000),
        "stdout_sha256": sha256_bytes(result.stdout),
        "stderr_sha256": sha256_bytes(result.stderr),
    }


def state_schema(codewhale_home: Path) -> dict[str, Any]:
    database = codewhale_home / "state.db"
    require(database.is_file() and not database.is_symlink(), "state_database_missing")
    try:
        connection = sqlite3.connect(f"file:{database}?mode=ro", uri=True)
        row = connection.execute("PRAGMA user_version").fetchone()
        quick = connection.execute("PRAGMA quick_check").fetchall()
        foreign = connection.execute("PRAGMA foreign_key_check").fetchall()
    except sqlite3.Error as error:
        raise EvaluationError("state_database_invalid") from error
    finally:
        if "connection" in locals():
            connection.close()
    version = row[0] if row else None
    require(
        version == STATE_SCHEMA and quick == [("ok",)] and not foreign,
        "state_database_invalid",
    )
    return {"valid": True, "version": version}


def tree_contains(root: Path, needle: bytes) -> bool:
    for path in root.rglob("*"):
        if path.is_file() and not path.is_symlink():
            try:
                if needle in path.read_bytes():
                    return True
            except OSError as error:
                raise EvaluationError("secret_scan_failed") from error
    return False


def tool_outcome_success(outcome: Any) -> bool:
    return bool(
        isinstance(outcome, dict)
        and outcome.get("invocation") == "accepted"
        and outcome.get("transport") == "succeeded"
        and outcome.get("operation") == "succeeded"
    )


def audit_lifecycle(
    variant: str,
    root_events: list[dict[str, Any]],
    child_runs: list[dict[str, Any]],
) -> dict[str, Any]:
    positions: dict[str, list[int]] = {}
    for index, stored in enumerate(root_events):
        positions.setdefault(event_kind(stored), []).append(index)
    prepared = [
        stored["event"]
        for stored in root_events
        if event_kind(stored) == "agent_task_prepared"
    ]
    started = positions.get("child_started", [])
    finished = positions.get("child_finished", [])
    agent_tools = [
        stored["event"]
        for stored in root_events
        if event_kind(stored) == "tool_prepared"
        and stored["event"].get("invocation", {}).get("name") == "agent"
    ]
    handoffs = [
        stored["event"].get("handoff_content")
        for stored in root_events
        if event_kind(stored) == "child_finished"
    ]
    if variant == "control":
        valid = not prepared and not started and not finished and not agent_tools
        reasons = [] if valid else ["control_must_have_no_child_surface"]
        return {
            "valid": valid,
            "reasons": reasons,
            "agent_calls": len(agent_tools),
            "children": 0,
            "same_batch_overlap": False,
            "handoffs_complete": False,
        }
    reasons: list[str] = []
    if len(agent_tools) != 2:
        reasons.append("exactly_two_agent_calls_required")
    if len(prepared) != 2 or len(started) != 2 or len(finished) != 2:
        reasons.append("two_child_lifecycles_required")
    if len(started) == 2 and len(finished) == 2 and started[1] >= finished[0]:
        reasons.append("children_not_launched_in_same_batch")
    if any(
        event.get("task", {}).get("workspace", {}).get("access") != "read_only"
        for event in prepared
    ):
        reasons.append("child_not_read_only")
    if len(child_runs) != 2 or any(
        run.get("terminal", {}).get("state") != "completed"
        for run in child_runs
    ):
        reasons.append("child_terminal_incomplete")
    if len(handoffs) != 2 or any(
        not isinstance(content, str) or not content.strip() for content in handoffs
    ):
        reasons.append("typed_handoff_incomplete")
    return {
        "valid": not reasons,
        "reasons": reasons,
        "agent_calls": len(agent_tools),
        "children": len(prepared),
        "same_batch_overlap": (
            len(started) == 2 and len(finished) == 2 and started[1] < finished[0]
        ),
        "handoffs_complete": (
            len(handoffs) == 2
            and all(isinstance(content, str) and content.strip() for content in handoffs)
        ),
    }


def run_identity(root_events: list[dict[str, Any]], task_id: str) -> dict[str, Any]:
    created = [
        stored["event"]["request"]
        for stored in root_events
        if event_kind(stored) == "run_created"
    ]
    require(len(created) == 1, "run_created_invalid")
    request = created[0]
    expected_definition = task_definition(task_id)
    identity_checks = {
        "model": request.get("model") == MODEL,
        "reasoning_effort": request.get("reasoning_effort") == "high",
        "max_output_tokens": request.get("max_output_tokens") == MAX_OUTPUT_TOKENS,
        "task_definition": request.get("task_contract", {}).get("definition")
        == expected_definition,
    }
    require(
        all(identity_checks.values()),
        "run_identity_invalid",
        {
            "checks": identity_checks,
            "expected_task_definition_sha256": canonical_hash(expected_definition),
            "actual_task_definition_sha256": canonical_hash(
                request.get("task_contract", {}).get("definition")
            ),
        },
    )
    prepared = [
        stored["event"]["request"]
        for stored in root_events
        if event_kind(stored) == "model_request_prepared"
    ]
    require(bool(prepared), "model_request_identity_missing")
    catalogs = [
        {
            "sha256": canonical_hash(request.get("tools")),
            "names": [
                tool.get("name")
                for tool in request.get("tools", [])
                if isinstance(tool, dict)
            ],
        }
        for request in prepared
    ]
    return {
        "task_definition_sha256": canonical_hash(
            request["task_contract"]["definition"]
        ),
        "root_request_count": len(prepared),
        "catalogs": catalogs,
        "execution_fingerprint_sha256": request.get("environment", {}).get(
            "execution_fingerprint_sha256"
        ),
        "tool_catalog_sha256": request.get("environment", {}).get(
            "tool_catalog_sha256"
        ),
    }


def execute_arm(
    binary: Path,
    binary_identity: dict[str, Any],
    key: str,
    schedule: dict[str, Any],
) -> dict[str, Any]:
    task_id = schedule["task_id"]
    variant = schedule["variant"]
    evaluation_id = uuid.uuid4().hex
    started_at = time.monotonic()
    secret = key.encode("utf-8")
    with tempfile.TemporaryDirectory(prefix="codewhale-m7g-arm-") as raw_temp:
        arm_root = Path(raw_temp)
        workspace = arm_root / "workspace"
        fixture_sha256 = materialize_fixture(task_id, workspace)
        state_root = arm_root / "state"
        home = state_root / "home"
        codewhale_home = state_root / "codewhale"
        xdg = state_root / "xdg"
        for directory in (state_root, home, codewhale_home, xdg):
            directory.mkdir(parents=True, exist_ok=True)
        environment = {
            **safe_env(),
            "HOME": str(home),
            "CODEWHALE_HOME": str(codewhale_home),
            "XDG_CONFIG_HOME": str(xdg),
            "DEEPSEEK_API_KEY": key,
        }
        stderr_path = state_root / "app-server.stderr"
        with stderr_path.open("wb") as stderr_stream:
            try:
                process = subprocess.Popen(
                    [
                        str(binary),
                        "--provider",
                        "deepseek",
                        "app-server",
                        "--stdio",
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
        client = StdioClient(process, secret)
        failure: EvaluationError | None = None
        run: dict[str, Any] = {}
        root_events: list[dict[str, Any]] = []
        child_runs: list[dict[str, Any]] = []
        try:
            result = client.call(
                start_envelope(
                    task_id,
                    variant,
                    workspace,
                    f"start-{evaluation_id}",
                ),
                30,
            )
            require(result.get("kind") == "run", "start_run_missing")
            run = result.get("run")
            require(isinstance(run, dict), "start_run_missing")
            deadline = time.monotonic() + WALL_TIME_SECONDS + HARNESS_GRACE_SECONDS
            run = wait_terminal(client, run, deadline, evaluation_id)
            run_id = run.get("run_id")
            require(isinstance(run_id, str), "run_id_missing")
            root_events = fetch_events(client, run_id, f"{evaluation_id}-root")
            child_ids = [
                stored["event"].get("task", {}).get("child_run_id")
                for stored in root_events
                if event_kind(stored) == "agent_task_prepared"
            ]
            for index, child_id in enumerate(child_ids):
                require(isinstance(child_id, str), "child_id_invalid")
                child_result = client.call(
                    query_envelope(
                        "get", child_id, f"child-{evaluation_id}-{index}"
                    )
                )
                require(child_result.get("kind") == "run", "child_run_missing")
                child = child_result.get("run")
                require(isinstance(child, dict), "child_run_missing")
                child_runs.append(child)
        except EvaluationError as error:
            failure = error
        finally:
            stop_process(process)

        stderr = stderr_path.read_bytes() if stderr_path.exists() else b""
        require(secret not in stderr, "key_in_stderr")
        require(secret not in canonical_bytes(run), "key_in_run_projection")
        require(not tree_contains(arm_root, secret), "key_in_local_artifact")
        if failure is not None:
            raise failure

        terminal = run.get("terminal", {})
        state_identity = state_schema(codewhale_home)
        accounting = accounting_projection(run)
        try:
            identity = run_identity(root_events, task_id)
        except EvaluationError as error:
            raise EvaluationError(
                error.code,
                {
                    **error.details,
                    "terminal_state": terminal.get("state"),
                    "accounting": accounting,
                    "state_schema": state_identity,
                },
            ) from error
        lifecycle = audit_lifecycle(variant, root_events, child_runs)
        verifier = external_verifier(workspace)
        changed = changed_files(workspace)
        expected_changed = [TASKS[task_id]["target"]]
        host_verifications = [
            stored["event"]
            for stored in root_events
            if event_kind(stored) == "host_verification_committed"
        ]
        host_receipt = any(
            event.get("receipt") is not None
            and tool_outcome_success(event.get("outcome"))
            for event in host_verifications
        )
        terminal_completed = terminal.get("state") == "completed"
        verified_success = (
            terminal_completed
            and verifier["passed"]
            and changed == expected_changed
            and host_receipt
            and lifecycle["valid"]
        )
        false_success = terminal_completed and not verified_success
        tool_outcomes = [
            stored["event"]
            for stored in root_events
            if event_kind(stored) == "tool_outcome_committed"
        ]
        failure_codes = Counter(
            event.get("outcome", {}).get("failure_code")
            or "missing_failure_code"
            for event in tool_outcomes
            if not tool_outcome_success(event.get("outcome"))
        )
        wall_time_ms = int((time.monotonic() - started_at) * 1000)
        require(
            accounting["cost_usd"] <= PER_ARM_COST_CEILING_USD,
            "arm_cost_ceiling_exceeded",
            {"cost_usd": accounting["cost_usd"]},
        )
        return {
            "record_type": "arm",
            "evaluation_id": evaluation_id,
            **schedule,
            "binary": binary_identity,
            "fixture_sha256": fixture_sha256,
            "task_definition_sha256": canonical_hash(task_definition(task_id)),
            "terminal_state": terminal.get("state"),
            "terminal_completed": terminal_completed,
            "verified_success": verified_success,
            "false_success": false_success,
            "external_verifier": verifier,
            "changed_files": changed,
            "expected_changed_files": expected_changed,
            "host_receipt": host_receipt,
            "completion_rejections": sum(
                event_kind(stored) == "completion_rejected"
                for stored in root_events
            ),
            "tool_calls": run.get("tool_calls"),
            "failed_tool_outcomes": sum(failure_codes.values()),
            "failure_codes": dict(sorted(failure_codes.items())),
            "lifecycle": lifecycle,
            "identity": identity,
            "accounting": accounting,
            "wall_time_ms": wall_time_ms,
            "stderr_sha256": sha256_bytes(stderr),
            "state_schema": state_identity,
            "key_accessed": True,
            "maximum_reruns": 0,
        }


def probe_binary(binary: Path, revision: str) -> dict[str, Any]:
    require(binary.is_file() and not binary.is_symlink(), "binary_unavailable")
    result = run_command([str(binary), "--version"], cwd=ROOT, timeout=15)
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


def preflight(binary: Path, revision: str, *, require_clean: bool) -> dict[str, Any]:
    require(git_output("branch", "--show-current") == "deepseek-agent", "branch_invalid")
    require(git_output("rev-parse", "HEAD") == revision, "revision_invalid")
    if require_clean:
        require(not git_output("status", "--porcelain=v1"), "worktree_dirty")
    manifest = load_manifest(frozen=True)
    identity = probe_binary(binary, revision)
    task_hashes = {
        task_id: fixture_hash(FIXTURE_ROOT / task_id) for task_id in TASKS
    }
    expected_hashes = {
        task["id"]: task["fixture_tree_sha256"]
        for task in manifest["tasks"]
    }
    require(task_hashes == expected_hashes, "fixture_hash_mismatch")
    return {
        "manifest_sha256": file_hash(MANIFEST_PATH),
        "harness_sha256": file_hash(Path(__file__).resolve()),
        "schedule_sha256": canonical_hash(formal_schedule()),
        "task_contracts_sha256": canonical_hash(
            {task_id: task_definition(task_id) for task_id in TASKS}
        ),
        "fixture_hashes": task_hashes,
        "binary": identity,
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


def emit(stream: BinaryIO, record: dict[str, Any]) -> None:
    line = canonical_bytes({"schema": SCHEMA, **record}) + b"\n"
    stream.write(line)
    stream.flush()
    os.fsync(stream.fileno())


def reserve_output(path: Path) -> BinaryIO:
    require(path.is_absolute(), "output_must_be_absolute")
    require(path.parent.resolve() == (ROOT / "eval/results").resolve(), "output_scope_invalid")
    descriptor = os.open(
        path,
        os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0),
        0o600,
    )
    os.fchmod(descriptor, 0o600)
    return os.fdopen(descriptor, "wb", buffering=0)


def paired_summary(arms: list[dict[str, Any]]) -> dict[str, Any]:
    pairs: dict[tuple[str, int], dict[str, dict[str, Any]]] = {}
    for arm in arms:
        pairs.setdefault((arm["task_id"], arm["run_index"]), {})[
            arm["variant"]
        ] = arm
    require(
        len(pairs) == 9 and all(set(pair) == set(VARIANTS) for pair in pairs.values()),
        "paired_matrix_incomplete",
    )
    deltas = []
    for (task_id, run_index), pair in sorted(pairs.items()):
        control = pair["control"]
        treatment = pair["treatment"]
        require(
            control["task_definition_sha256"]
            == treatment["task_definition_sha256"],
            "paired_task_identity_mismatch",
        )
        deltas.append(
            {
                "task_id": task_id,
                "run_index": run_index,
                "wall_time_ms": treatment["wall_time_ms"]
                - control["wall_time_ms"],
                "requests": treatment["accounting"]["requests"]
                - control["accounting"]["requests"],
                "input_tokens": treatment["accounting"]["tokens"]["input"]
                - control["accounting"]["tokens"]["input"],
                "cost_nanousd": treatment["accounting"]["cost_nanousd"]
                - control["accounting"]["cost_nanousd"],
                "control_verified": control["verified_success"],
                "treatment_verified": treatment["verified_success"],
            }
        )
    wall = [delta["wall_time_ms"] for delta in deltas]
    return {
        "pairs": deltas,
        "wall_time_wins": sum(value < 0 for value in wall),
        "wall_time_losses": sum(value > 0 for value in wall),
        "wall_time_ties": sum(value == 0 for value in wall),
        "wall_time_mean_delta_ms": statistics.mean(wall),
        "wall_time_median_delta_ms": statistics.median(wall),
    }


def aggregate(arms: list[dict[str, Any]]) -> dict[str, Any]:
    paired = paired_summary(arms)
    variants: dict[str, Any] = {}
    for variant in VARIANTS:
        selected = [arm for arm in arms if arm["variant"] == variant]
        variants[variant] = {
            "arms": len(selected),
            "verified_success": sum(arm["verified_success"] for arm in selected),
            "false_success": sum(arm["false_success"] for arm in selected),
            "fanout_valid": sum(arm["lifecycle"]["valid"] for arm in selected),
            "wall_time_mean_ms": statistics.mean(
                arm["wall_time_ms"] for arm in selected
            ),
            "requests_mean": statistics.mean(
                arm["accounting"]["requests"] for arm in selected
            ),
            "input_tokens_mean": statistics.mean(
                arm["accounting"]["tokens"]["input"] for arm in selected
            ),
            "cost_usd_mean": statistics.mean(
                arm["accounting"]["cost_usd"] for arm in selected
            ),
        }
    control = variants["control"]
    treatment = variants["treatment"]
    reliable = (
        treatment["verified_success"] >= control["verified_success"]
        and treatment["false_success"] <= control["false_success"]
    )
    fanout_complete = treatment["fanout_valid"] == 9
    wall_ratio = (
        treatment["wall_time_mean_ms"] / control["wall_time_mean_ms"]
        if control["wall_time_mean_ms"]
        else None
    )
    stable_wall_gain = (
        wall_ratio is not None
        and wall_ratio <= 0.9
        and paired["wall_time_wins"] >= 6
        and paired["wall_time_median_delta_ms"] < 0
    )
    decision = (
        "keep_explicit"
        if reliable and fanout_complete and stable_wall_gain
        else "hold"
    )
    return {
        "record_type": "summary",
        "product_metric_eligible": True,
        "variants": variants,
        "paired": paired,
        "wall_time_ratio_treatment_over_control": wall_ratio,
        "reliable": reliable,
        "fanout_complete": fanout_complete,
        "stable_wall_gain": stable_wall_gain,
        "decision": decision,
        "key_accessed": True,
        "maximum_reruns": 0,
    }


def plan_record(identity: dict[str, Any]) -> dict[str, Any]:
    return {
        "record_type": "plan",
        "product_metric_eligible": False,
        "source_identity": identity,
        "model": MODEL,
        "reasoning_effort": "high",
        "variants": {
            "control": {
                "agent_advertised": False,
                "max_depth": 0,
                "max_concurrent_children": 0,
            },
            "treatment": {
                "agent_advertised": True,
                "required_read_only_children": 2,
                "max_depth": 1,
                "max_concurrent_children": 2,
            },
        },
        "same_across_arms": {
            "binary": identity["binary"],
            "model": MODEL,
            "reasoning_effort": "high",
            "max_output_tokens": MAX_OUTPUT_TOKENS,
            "max_api_requests": MAX_API_REQUESTS,
            "max_turns": MAX_TURNS,
            "max_tool_calls": MAX_TOOL_CALLS,
            "maximum_reruns": 0,
        },
        "schedule": formal_schedule(),
        "arms": 18,
        "pairs": 9,
        "suite_cost_ceiling_usd": 18 * PER_ARM_COST_CEILING_USD,
        "key_accessed": False,
        "network_accessed": False,
    }


def run_self_test() -> int:
    manifest = load_manifest(frozen=False)
    schedule = formal_schedule()
    require(len(schedule) == 18, "self_test_schedule_length")
    require(
        Counter(item["variant"] for item in schedule)
        == Counter({"control": 9, "treatment": 9}),
        "self_test_schedule_balance",
    )
    require(
        all(
            task_definition(task_id) == task_definition(task_id)
            for task_id in TASKS
        ),
        "self_test_task_determinism",
    )
    require(
        manifest["resources"]["maximum_reruns"] == 0,
        "self_test_rerun_contract",
    )
    verifier = verifier_spec("t1-capability-intersection")
    verifier_step = verifier["plan"]["steps"][0]
    require(
        verifier_step["env"] == HOST_VERIFIER_ENV
        and verifier_step["timeout_ms"] == HOST_VERIFIER_TIMEOUT_MS,
        "self_test_host_verifier_identity",
    )
    print(
        json.dumps(
            {
                "schema": SCHEMA,
                "record_type": "self_test",
                "passed": True,
                "schedule_sha256": canonical_hash(schedule),
                "task_contracts_sha256": canonical_hash(
                    {
                        task_id: task_definition(task_id)
                        for task_id in TASKS
                    }
                ),
            },
            ensure_ascii=False,
            sort_keys=True,
        )
    )
    return 0


def run_freeze_report() -> int:
    manifest = load_manifest(frozen=False)
    without_hashes = dict(manifest)
    without_hashes.pop("frozen_hashes", None)
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
                    task_id: fixture_hash(FIXTURE_ROOT / task_id)
                    for task_id in TASKS
                },
                "manifest_content_sha256_excluding_frozen_hashes": canonical_hash(
                    without_hashes
                ),
            },
            ensure_ascii=False,
            indent=2,
            sort_keys=True,
        )
    )
    return 0


def run_dry(args: argparse.Namespace) -> int:
    revision = args.revision or git_output("rev-parse", "HEAD")
    identity = preflight(Path(args.binary).resolve(), revision, require_clean=False)
    print(json.dumps(plan_record(identity), ensure_ascii=False, sort_keys=True))
    return 0


def run_formal(args: argparse.Namespace) -> int:
    require(args.acknowledge_cost, "cost_acknowledgement_required")
    require(args.key_file, "key_file_required")
    require(args.output, "output_required")
    revision = args.revision or git_output("rev-parse", "HEAD")
    binary = Path(args.binary).resolve()
    identity = preflight(binary, revision, require_clean=True)
    output_path = Path(args.output).resolve()
    with reserve_output(output_path) as output:
        emit(output, plan_record(identity))
        key = read_key(Path(args.key_file).expanduser().resolve())
        emit(
            output,
            {
                "record_type": "credential_access",
                "key_accessed": True,
                "network_accessed": False,
            },
        )
        frozen_root = Path(
            tempfile.mkdtemp(prefix="codewhale-m7g-binary-")
        )
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
                        frozen_binary, frozen_identity, key, scheduled
                    )
                except EvaluationError as error:
                    emit(
                        output,
                        {
                            "record_type": "abort",
                            "error_code": error.code,
                            "details": error.details,
                            "completed_arms": len(arms),
                            "key_accessed": True,
                            "network_accessed": True,
                            "maximum_reruns": 0,
                        },
                    )
                    return 2
                arms.append(arm)
                emit(output, arm)
            summary = aggregate(arms)
            emit(output, summary)
            return 0
        finally:
            key = ""
            try:
                frozen_binary.unlink(missing_ok=True)
                frozen_root.rmdir()
            except OSError:
                pass


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--self-test", action="store_true")
    mode.add_argument("--freeze-report", action="store_true")
    mode.add_argument("--dry-run", action="store_true")
    parser.add_argument("--binary")
    parser.add_argument("--revision")
    parser.add_argument("--acknowledge-cost", action="store_true")
    parser.add_argument("--key-file")
    parser.add_argument("--output")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
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
                    "schema": SCHEMA,
                    "record_type": "error",
                    "error_code": error.code,
                    "details": error.details,
                    "key_accessed": False,
                },
                ensure_ascii=False,
                sort_keys=True,
            ),
            file=sys.stderr,
        )
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
