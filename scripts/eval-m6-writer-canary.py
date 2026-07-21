#!/usr/bin/env python3
"""Cost-bounded production canary for the M6 isolated-writer vertical slice."""

from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
import re
import selectors
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import time
import uuid
from pathlib import Path
from typing import Any


SCHEMA, RUN_API, EVENT_API = "codewhale.eval.m6-writer-canary.v1", 9, 13
MODEL, MODELS = "deepseek-v4-flash", ("deepseek-v4-flash", "deepseek-v4-pro")
FILE, BEFORE, AFTER = "answer.txt", b"before\n", b"after\n"
MAX_REQUESTS, RUNTIME_SECONDS = 10, 420
HARNESS_SECONDS = RUNTIME_SECONDS + 15
MAX_POLLS, MAX_STDIO_FRAME = 2_200, 4 * 1024 * 1024
ROOT_BRANCH_REF = "refs/heads/main"
ROOT_TOOLS = ["agent", "read_file", "apply_patch", "edit_file"]
WRITER_TOOLS = ["read_file", "apply_patch", "edit_file"]
EXPECTED_ARTIFACT = "Host seal 的单文件提交和冻结验证证据"
ENV_ALLOWLIST = "PATH TMPDIR LANG LC_ALL LC_CTYPE".split()
NETWORK_OVERRIDE_ENV = """SSL_CERT_FILE SSL_CERT_DIR REQUESTS_CA_BUNDLE CURL_CA_BUNDLE
NODE_EXTRA_CA_CERTS AWS_CA_BUNDLE GIT_SSL_CAINFO HTTPS_PROXY HTTP_PROXY ALL_PROXY
NO_PROXY https_proxy http_proxy all_proxy no_proxy""".split()
VERSION_PATTERN = re.compile(
    r"^codewhale (?P<version>[0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.-]+)?) "
    r"\((?P<revision>[0-9a-f]{12})\)$"
)
VERIFY_CODE = (
    "from pathlib import Path\n"
    f"p=Path({FILE!r})\n"
    f"raise SystemExit(0 if p.is_file() and not p.is_symlink() "
    f"and p.read_bytes()=={AFTER!r} else 1)\n"
)
OBJECTIVE = f"""完成一个最小隔离写入任务。

全程必须且只能调用一次 agent 工具；根 Agent 不得直接写文件，可在委派前后只读核对。调用参数必须是：
type=implementer、workspace_access=isolated_write、allowed_paths=["{FILE}"]、
fork_context=false、allowed_tools=["read_file","apply_patch","edit_file"]、
max_steps=4、max_depth=0、wall_time_secs=180、
expected_artifact="{EXPECTED_ARTIFACT}"。
子 Agent 只把 {FILE} 从 before 改为 after，保留末尾换行。集成后不再调用工具。
"""

class Failure(RuntimeError):
    def __init__(self, code: str, details: dict[str, Any] | None = None) -> None:
        super().__init__(code)
        self.code = code
        self.details = details or {}

def digest(value: bytes) -> str:
    return "sha256:" + hashlib.sha256(value).hexdigest()

def json_digest(value: Any) -> str:
    encoded = json.dumps(
        value, ensure_ascii=False, sort_keys=True, separators=(",", ":")
    ).encode()
    return digest(encoded)

def check(condition: bool, code: str) -> None:
    if not condition:
        raise Failure(code)


def fields(value: Any, code: str, **expected: Any) -> None:
    check(
        isinstance(value, dict) and all(value.get(key) == item for key, item in expected.items()),
        code,
    )

def safe_env() -> dict[str, str]:
    env = {key: os.environ[key] for key in ENV_ALLOWLIST if key in os.environ}
    env.update(NO_COLOR="1", GIT_CONFIG_NOSYSTEM="1", RUST_BACKTRACE="0", RUST_LOG="off")
    return env

def git(workspace: Path, *args: str) -> str:
    result = subprocess.run(
        ["git", *args],
        cwd=workspace,
        env=safe_env(),
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        timeout=30,
        check=False,
        text=True,
    )
    check(result.returncode == 0, "git_failed")
    return result.stdout.strip()

def make_workspace(root: Path) -> tuple[Path, str]:
    workspace = root / "workspace"
    workspace.mkdir()
    (workspace / FILE).write_bytes(BEFORE)
    git(workspace, "init", "-q", "-b", "main")
    git(workspace, "add", "--", FILE)
    result = subprocess.run(
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
            "writer canary fixture",
        ],
        cwd=workspace,
        env={
            **safe_env(),
            "GIT_AUTHOR_DATE": "2026-07-20T00:00:00Z",
            "GIT_COMMITTER_DATE": "2026-07-20T00:00:00Z",
        },
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        timeout=30,
        check=False,
    )
    check(not result.returncode and not git(workspace, "status", "--porcelain=v1"), "fixture_not_clean")
    base = git(workspace, "rev-parse", "HEAD")
    check(
        len(base) == 40
        and git(workspace, "symbolic-ref", "-q", "HEAD") == ROOT_BRANCH_REF,
        "fixture_identity_invalid",
    )
    return workspace, base

def read_key(path: Path) -> str:
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
    try:
        fd = os.open(path, flags)
    except OSError as error:
        raise Failure("key_unavailable") from error
    try:
        meta = os.fstat(fd)
        check(stat.S_ISREG(meta.st_mode), "key_not_regular")
        check(stat.S_IMODE(meta.st_mode) == 0o600, "key_mode_not_0600")
        check(0 < meta.st_size <= 16_384, "key_size_invalid")
        raw = os.read(fd, 16_385)
        check(len(raw) == meta.st_size, "key_size_changed")
        value = raw.decode().strip()
    except Failure:
        raise
    except (OSError, UnicodeDecodeError) as error:
        raise Failure("key_unreadable") from error
    finally:
        os.close(fd)
    check(
        bool(value) and not any(character.isspace() or ord(character) < 32 for character in value),
        "key_format_invalid",
    )
    return value


def file_contains(path: Path, needle: bytes) -> bool:
    tail = b""
    try:
        with path.open("rb") as stream:
            while chunk := stream.read(1024 * 1024):
                block = tail + chunk
                if needle in block:
                    return True
                tail = block[-max(len(needle) - 1, 0):]
    except OSError as error:
        raise Failure("secret_scan_failed") from error
    return False


def tree_contains(root: Path, needle: bytes) -> bool:
    try:
        for directory, names, files in os.walk(root, followlinks=False):
            names[:] = [
                name
                for name in names
                if not (Path(directory) / name).is_symlink()
            ]
            for name in files:
                path = Path(directory) / name
                metadata = path.lstat()
                if stat.S_ISREG(metadata.st_mode) and file_contains(path, needle):
                    return True
    except OSError as error:
        raise Failure("secret_scan_failed") from error
    return False


def bounded_line(stream: Any) -> bytes:
    line = stream.readline(MAX_STDIO_FRAME + 1)
    check(
        bool(line) and line.endswith(b"\n") and len(line) <= MAX_STDIO_FRAME,
        "stdio_frame_invalid",
    )
    return line


def parse_binary_version(line: bytes, candidate_revision: str) -> dict[str, str]:
    try:
        text = line.decode("utf-8").strip()
    except UnicodeDecodeError as error:
        raise Failure("candidate_version_invalid") from error
    matched = VERSION_PATTERN.fullmatch(text)
    check(
        matched is not None and matched.group("revision") == candidate_revision[:12],
        "candidate_version_invalid",
    )
    return {"version": matched.group("version"), "revision_prefix": matched.group("revision")}


def probe_binary(
    binary: Path, workspace: Path, environment: dict[str, str], revision: str
) -> dict[str, str]:
    check(
        not any(name in environment for name in NETWORK_OVERRIDE_ENV)
        and "DEEPSEEK_API_KEY" not in environment,
        "candidate_probe_environment_invalid",
    )
    try:
        process = subprocess.Popen(
            [str(binary), "--version"], cwd=workspace, env=environment,
            stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
            start_new_session=True,
        )
    except OSError as error:
        raise Failure("candidate_probe_failed") from error
    selector = selectors.DefaultSelector()
    try:
        check(process.stdout is not None, "candidate_probe_failed")
        selector.register(process.stdout, selectors.EVENT_READ)
        check(bool(selector.select(5)), "candidate_probe_timeout")
        identity = parse_binary_version(bounded_line(process.stdout), revision)
        process.wait(timeout=5)
        check(process.returncode == 0 and process.stdout.read(1) == b"", "candidate_probe_failed")
        return identity
    except subprocess.TimeoutExpired as error:
        raise Failure("candidate_probe_timeout") from error
    finally:
        selector.close()
        if process.poll() is None:
            stop(process)


def verifier(python: Path) -> dict[str, Any]:
    program = str(python.resolve())
    arguments = ["-I", "-c", VERIFY_CODE]
    command = {"name": "writer-answer-exact", "program": program, "args": arguments, "cwd": ""}
    step = {**command, "id": command["name"], "env": {}, "timeout_ms": 600_000}
    step.pop("name")
    return {
        "verifier_id": "run_verifiers",
        "parameters": {"profile": "exact", "level": "quick", "max_python_files": 200, "commands": [command]},
        "plan": {"steps": [step]},
    }

def task(python: Path) -> dict[str, Any]:
    verify = {
        "kind": "verifier",
        "id": "writer-answer",
        "description": f"{FILE} 精确等于 after 并保留末尾换行",
        "verifier": verifier(python),
    }
    return {
        "objective": OBJECTIVE,
        "constraints": [f"根 Agent 只能调用一次 agent 且不得直接写；writer 只允许修改 {FILE}", "必须由 Host seal、确定性验证和 fast-forward 完成集成"],
        "non_goals": ["根 Agent 直接编辑", "第二个子 Agent", "修改 Git 元数据"],
        "acceptance": [verify],
    }

def start_command(workspace: Path, python: Path, model: str, request_id: str) -> dict[str, Any]:
    limits = {
        "max_turns": 8, "max_model_requests": 8, "max_model_retries": 1,
        "max_tool_calls": 8, "max_depth": 1, "max_concurrent_children": 1,
        "model_event_idle_ms": 120_000, "wall_time_ms": RUNTIME_SECONDS * 1000,
    }
    return {
        "schema_version": RUN_API,
        "request_id": request_id,
        "command": {
            "kind": "start",
            "task": task(python),
            "workspace": str(workspace.resolve()),
            "model": model,
            "reasoning_effort": "high",
            "max_output_tokens": 8192,
            "max_api_requests": MAX_REQUESTS,
            "streaming": True,
            "tool_policy": {"enabled": True, "allowed": ROOT_TOOLS, "denied": []},
            "limits": limits,
            "controls": {
                "write_execution_mode": "isolated_writer",
                "auto_approve": True,
                "trust_mode": False,
                "allow_sandbox_elevation": False,
                "interactive": False,
                "sandbox": "workspace-write",
            },
        },
    }

def query(kind: str, run_id: str, request_id: str) -> dict[str, Any]:
    command: dict[str, Any] = {"kind": kind, "run_id": run_id}
    if kind == "events":
        command["after_sequence"] = 0
    return {"schema_version": RUN_API, "request_id": request_id, "command": command}

class Stdio:
    def __init__(self, process: subprocess.Popen[bytes], forbidden: bytes) -> None:
        if process.stdin is None or process.stdout is None:
            raise Failure("stdio_missing")
        self.process = process
        self.stdin = process.stdin
        self.stdout = process.stdout
        self.forbidden = forbidden
        self.stdin_fd = self.stdin.fileno()
        self.stdout_fd = self.stdout.fileno()
        os.set_blocking(self.stdin_fd, False)
        os.set_blocking(self.stdout_fd, False)
        self.buffer = bytearray()
        self.selector = selectors.DefaultSelector()
        self.selector.register(self.stdout_fd, selectors.EVENT_READ)

    def call(
        self,
        envelope: dict[str, Any],
        timeout_seconds: float = 30,
    ) -> dict[str, Any]:
        request_id = envelope["request_id"]
        encoded = (
            json.dumps(envelope, ensure_ascii=False, separators=(",", ":")).encode()
            + b"\n"
        )
        check(self.forbidden not in encoded, "key_in_protocol")
        deadline = time.monotonic() + max(0.0, timeout_seconds)
        self._write_frame(encoded, deadline)
        line = self._read_frame(deadline)
        check(self.forbidden not in line, "key_in_protocol")
        try:
            response = json.loads(line)
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise Failure("stdio_json_invalid") from error
        check(
            isinstance(response, dict)
            and response.get("schema_version") == RUN_API
            and response.get("request_id") == request_id
            and isinstance(response.get("result"), dict),
            "stdio_response_invalid",
        )
        result = response["result"]
        if result.get("kind") == "error":
            code = result.get("error", {}).get("code", "unknown")
            raise Failure(f"run_api_{code}" if isinstance(code, str) else "run_api_unknown")
        return result

    def _write_frame(self, encoded: bytes, deadline: float) -> None:
        view = memoryview(encoded)
        while view:
            try:
                written = os.write(self.stdin_fd, view)
            except BlockingIOError:
                written = 0
            except OSError as error:
                raise Failure("stdio_write_failed") from error
            if written:
                view = view[written:]
                continue
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise Failure("stdio_timeout")
            with selectors.DefaultSelector() as writable:
                writable.register(self.stdin_fd, selectors.EVENT_WRITE)
                if not writable.select(remaining):
                    raise Failure("stdio_timeout")

    def _read_frame(self, deadline: float) -> bytes:
        while True:
            newline = self.buffer.find(b"\n")
            if newline >= 0:
                line = bytes(self.buffer[: newline + 1])
                del self.buffer[: newline + 1]
                check(len(line) <= MAX_STDIO_FRAME, "stdio_frame_invalid")
                return line
            check(len(self.buffer) <= MAX_STDIO_FRAME, "stdio_frame_invalid")
            remaining = deadline - time.monotonic()
            if remaining <= 0 or not self.selector.select(remaining):
                raise Failure("stdio_timeout")
            try:
                chunk = os.read(self.stdout_fd, 65_536)
            except BlockingIOError:
                continue
            except OSError as error:
                raise Failure("stdio_read_failed") from error
            check(bool(chunk), "stdio_frame_invalid")
            self.buffer.extend(chunk)

    def close(self) -> None:
        self.selector.close()

def events(
    client: Stdio,
    run_id: str,
    request_id: str,
    timeout_seconds: float = 30,
) -> list[dict[str, Any]]:
    result = client.call(
        query("events", run_id, request_id),
        timeout_seconds=timeout_seconds,
    )
    value = result.get("events")
    check(
        result.get("kind") == "events" and result.get("run_id") == run_id and isinstance(value, list),
        "events_missing",
    )
    check(
        [event.get("sequence") for event in value] == list(range(1, len(value) + 1)),
        "event_sequence_invalid",
    )
    for stored in value:
        check(
            stored.get("schema_version") == EVENT_API and stored.get("run_id") == run_id,
            "event_envelope_invalid",
        )
    return value

def kind(stored: dict[str, Any]) -> str:
    return stored.get("event", {}).get("kind", "")

def event_summary(stream: list[dict[str, Any]]) -> dict[str, Any]:
    counts: dict[str, int] = {}
    kinds = [kind(stored) for stored in stream]
    for name in kinds:
        counts[name] = counts.get(name, 0) + 1
    failures = []
    tool_names = []
    tool_outcomes = []
    for stored in stream:
        event = stored.get("event", {})
        if event.get("kind") == "tool_prepared":
            tool_names.append(event.get("invocation", {}).get("name"))
        if event.get("kind") == "tool_outcome_committed":
            outcome = event.get("outcome", {})
            observation = outcome.get("verifier_observation", {})
            tool_outcomes.append(
                {
                    "name": event.get("name"),
                    "invocation": outcome.get("invocation"),
                    "operation": outcome.get("operation"),
                    "side_effect": outcome.get("side_effect"),
                    "verifier_verdict": observation.get("verdict"),
                }
            )
        if event.get("kind") != "model_request_failed":
            continue
        failure = event.get("failure", {})
        retry = event.get("retry", {})
        failures.append(
            {
                "sequence": stored.get("sequence"),
                "code": failure.get("code"),
                "category": failure.get("category"),
                "message": " ".join(str(failure.get("message", "")).split())[:512],
                "retryable": failure.get("retryable"),
                "actionable_output": failure.get("actionable_output"),
                "retry_decision": retry.get("decision"),
                "stop_reason": retry.get("reason"),
            }
        )
    summary = {
        "last_sequence": stream[-1].get("sequence") if stream else 0,
        "event_counts": counts,
        "event_tail": kinds[-32:],
    }
    if failures:
        summary["model_failures"] = failures
    if tool_names:
        summary["tool_names"] = tool_names
    if tool_outcomes:
        summary["tool_outcomes"] = tool_outcomes
    return summary

def run_summary(run: dict[str, Any]) -> dict[str, Any]:
    accounting_value = run.get("accounting", {})
    return {
        name: run.get(name)
        for name in (
            "run_id",
            "parent_run_id",
            "last_sequence",
            "terminal",
            "runtime_model_requests",
            "runtime_retries",
            "tool_calls",
            "local_turns",
        )
    } | {
        "accounting": {
            name: accounting_value.get(name)
            for name in (
                "root",
                "child",
                "hard_request_limit",
                "transport_retries",
                "complete",
                "usage_complete",
                "usage_missing",
                "usage_incomplete",
                "billing_unknown",
            )
        }
    }

def collect_progress(
    client: Stdio,
    root_id: str,
    run: dict[str, Any],
    request_suffix: str,
) -> dict[str, Any]:
    root_events = events(client, root_id, f"m6-debug-root-events-{request_suffix}")
    child_ids = [
        stored.get("event", {}).get("task", {}).get("child_run_id")
        for stored in root_events
        if kind(stored) == "agent_task_prepared"
    ]
    children = []
    for index, child_id in enumerate(child_ids):
        if not isinstance(child_id, str):
            continue
        result = client.call(query("get", child_id, f"m6-debug-child-get-{request_suffix}-{index}"))
        child = result.get("run") if result.get("kind") == "run" else {}
        child_events = events(
            client,
            child_id,
            f"m6-debug-child-events-{request_suffix}-{index}",
        )
        children.append(
            {
                "run": run_summary(child) if isinstance(child, dict) else {},
                "events": event_summary(child_events),
            }
        )
    return {
        "root": run_summary(run),
        "root_events": event_summary(root_events),
        "children": children,
    }

def one(stream: list[dict[str, Any]], name: str) -> dict[str, Any]:
    found = [stored["event"] for stored in stream if kind(stored) == name]
    check(len(found) == 1, f"expected_one_{name}")
    return found[0]

def ordered(stream: list[dict[str, Any]], names: list[str]) -> None:
    positions = []
    for name in names:
        found = [index for index, stored in enumerate(stream) if kind(stored) == name]
        check(len(found) == 1, f"expected_one_{name}")
        positions.append(found[0])
    check(positions == sorted(positions), "event_order_invalid")

def acceptance(definition: dict[str, Any]) -> dict[str, Any]:
    values = [
        value
        for value in definition.get("acceptance", [])
        if isinstance(value, dict) and value.get("kind") == "verifier"
    ]
    check(len(values) == 1, "one_verifier_required")
    return values[0]


def audit_run_identity(
    stream: list[dict[str, Any]],
    request: dict[str, Any],
    run_id: str,
    parent_id: str | None,
    model: str,
    streaming: bool,
    actor_kind: str,
    depth: int,
) -> None:
    fields(
        request, "run_identity_invalid", run_id=run_id, parent_run_id=parent_id,
        model=model, reasoning_effort="high", max_output_tokens=8192, streaming=streaming,
    )
    fields(request.get("environment"), "run_identity_invalid", provider="deepseek")
    fields(request.get("actor"), "run_identity_invalid", kind=actor_kind, depth=depth)
    prepared = [
        stored["event"]["request"]
        for stored in stream
        if kind(stored) == "model_request_prepared"
    ]
    check(bool(prepared), "model_request_identity_missing")
    for model_request in prepared:
        fields(
            model_request, "model_request_identity_invalid", run_id=run_id,
            parent_run_id=parent_id, model=model, reasoning_effort="high",
            max_output_tokens=8192, streaming=streaming,
        )
        fields(
            model_request.get("actor"), "model_request_identity_invalid",
            kind=actor_kind, depth=depth,
        )


def receipt(
    committed: dict[str, Any],
    terminal: dict[str, Any],
    expected: dict[str, Any],
    generation: str,
) -> dict[str, Any]:
    value = committed.get("receipt")
    outcome = committed.get("outcome", {})
    observation = outcome.get("verifier_observation", {})
    artifact_ids = value.get("artifact_ids") if isinstance(value, dict) else None
    available = {
        artifact.get("id")
        for artifact in outcome.get("artifacts", [])
        if isinstance(artifact, dict)
        and artifact.get("status") == "available"
        and isinstance(artifact.get("sha256"), str)
    }
    check(
        isinstance(value, dict)
        and value.get("generation_id") == generation
        and value.get("acceptance_id") == expected.get("id")
        and value.get("verifier") == expected.get("verifier")
        and value.get("workspace_state") == committed.get("workspace_state_after")
        and observation.get("verdict") == "passed"
        and observation.get("spec") == expected.get("verifier")
        and bool(artifact_ids)
        and observation.get("artifact_ids") == artifact_ids
        and set(artifact_ids) == available,
        "receipt_invalid",
    )
    decision = terminal.get("decision", {})
    satisfied = decision.get("satisfied", [])
    check(
        terminal.get("state") == "completed"
        and decision.get("workspace_state") == value.get("workspace_state")
        and len(satisfied) == 1
        and satisfied[0].get("kind") == "evidence"
        and satisfied[0].get("acceptance_id") == expected.get("id")
        and satisfied[0].get("receipt_id") == value.get("id"),
        "terminal_receipt_invalid",
    )
    return value


def check_post_integration_receipt(
    integrated_state: dict[str, Any], receipt_state: dict[str, Any]
) -> None:
    before = integrated_state.get("generation") if isinstance(integrated_state, dict) else None
    after = receipt_state.get("generation") if isinstance(receipt_state, dict) else None
    check(
        isinstance(before, int)
        and not isinstance(before, bool)
        and after == before + 1
        and receipt_state.get("revision") == integrated_state.get("revision"),
        "root_receipt_not_latest",
    )


def audit(
    root: list[dict[str, Any]],
    child: list[dict[str, Any]],
    expected_task: dict[str, Any],
    root_id: str,
    base: str,
    model: str,
) -> dict[str, Any]:
    created = one(root, "run_created")["request"]
    audit_run_identity(root, created, root_id, None, model, True, "root", 0)
    contract = created.get("task_contract", {})
    fields(contract, "root_contract_invalid", definition=expected_task, generation_id=root_id)
    expected_acceptance = acceptance(contract["definition"])

    prepared_tools = [stored["event"] for stored in root if kind(stored) == "tool_prepared"]
    prepared_tool_names = [
        event.get("invocation", {}).get("name")
        for event in prepared_tools
    ]
    agent_tools = [
        event
        for event in prepared_tools
        if event.get("invocation", {}).get("name") == "agent"
    ]
    direct_writes = [
        name
        for name in prepared_tool_names
        if name in {"apply_patch", "edit_file"}
    ]
    if (
        len(agent_tools) != 1
        or direct_writes
    ):
        raise Failure(
            "root_agent_call_invalid",
            {
                "agent_tool_count": len(agent_tools),
                "direct_write_tools": direct_writes,
                "prepared_tool_count": len(prepared_tools),
                "prepared_tool_names": prepared_tool_names,
            },
        )
    agent_call = agent_tools[0]
    args = agent_call.get("invocation", {}).get("arguments", {}).get("parsed", {})
    expected_args = {
        "type": "implementer",
        "workspace_access": "isolated_write",
        "allowed_paths": [FILE],
        "fork_context": False,
        "allowed_tools": WRITER_TOOLS,
        "max_steps": 4,
        "max_depth": 0,
        "wall_time_secs": 180,
        "expected_artifact": EXPECTED_ARTIFACT,
    }
    check(
        agent_call.get("workspace_access") == "may_write"
        and all(args.get(key) == value for key, value in expected_args.items())
        and FILE in args.get("prompt", ""),
        "root_agent_arguments_invalid",
    )

    agent_task = one(root, "agent_task_prepared")["task"]
    assignment = agent_task.get("workspace", {})
    child_id = agent_task.get("child_run_id")
    fields(agent_task, "agent_task_invalid", root_run_id=root_id, parent_run_id=root_id, role="implementer")
    fields(
        assignment, "agent_task_invalid", access="isolated_write", base_commit=base,
        root_workspace=created.get("environment", {}).get("workspace"),
        root_branch=ROOT_BRANCH_REF, allowed_paths=[FILE],
    )
    check(
        assignment.get("worktree_path") != assignment.get("root_workspace")
        and isinstance(child_id, str)
        and acceptance(agent_task.get("task_contract", {}).get("definition", {})) == expected_acceptance,
        "agent_task_invalid",
    )
    workspace_created = one(root, "agent_workspace_created")
    child_started = one(root, "child_started")
    fields(workspace_created, "writer_start_invalid", assignment=assignment)
    fields(child_started, "writer_start_invalid", child_run_id=child_id, depth=1)

    check(not any(stored.get("parent_run_id") != root_id for stored in child), "child_parent_invalid")
    child_request = one(child, "run_created")["request"]
    audit_run_identity(child, child_request, child_id, root_id, model, False, "child", 1)
    fields(child_request, "child_request_invalid", agent_task=agent_task)
    fields(
        child_request.get("environment"), "child_request_invalid",
        workspace=assignment.get("worktree_path"), sandbox="isolated_writer",
    )
    child_tools = [stored["event"] for stored in child if kind(stored) == "tool_prepared"]
    names = [value.get("invocation", {}).get("name") for value in child_tools]
    writes = [value for value in child_tools if value.get("workspace_access") == "may_write"]
    check(
        len(writes) == 1
        and writes[0].get("invocation", {}).get("name") in {"apply_patch", "edit_file"}
        and all(name in WRITER_TOOLS for name in names),
        "child_tools_invalid",
    )

    child_terminal = one(child, "terminal")["outcome"]["terminal"]
    child_receipt = receipt(
        one(child, "host_verification_committed"),
        child_terminal,
        expected_acceptance,
        child_id,
    )
    child_evidence = one(child, "terminal")["outcome"].get("details", {}).get("evidence")
    check(child_evidence == [child_receipt], "child_receipt_not_returned")
    seal = one(root, "agent_seal_committed")
    check(
        one(root, "agent_seal_prepared").get("writer_workspace_state_before")
        == child_receipt.get("workspace_state")
        and seal.get("final_commit") != base
        and seal.get("changed_files") == [FILE]
        and isinstance(seal.get("diff_sha256"), str)
        and re.fullmatch(r"[0-9a-f]{64}", seal["diff_sha256"])
        and seal.get("writer_workspace_state_after") != child_receipt.get("workspace_state"),
        "seal_invalid",
    )
    collected = one(root, "agent_result_collected").get("outcome", {}).get("details", {})
    check(
        collected.get("evidence") == [child_receipt]
        and collected.get("changed_files") == [FILE]
        and collected.get("final_commit") == seal.get("final_commit")
        and collected.get("integration", {}).get("state") == "awaiting_host",
        "collected_result_invalid",
    )

    integration = one(root, "agent_integration_committed")
    integration_prepared = one(root, "agent_integration_prepared")
    integration_started = one(root, "agent_integration_started")
    check(
        not any(kind(stored) == "agent_integration_failed" for stored in root)
        and integration_prepared.get("base_commit") == base
        and integration_prepared.get("writer_commit") == seal.get("final_commit")
        and integration_prepared.get("diff_sha256") == seal.get("diff_sha256")
        and integration_started.get("integration_id") == integration_prepared.get("integration_id")
        and integration.get("integration_id") == integration_prepared.get("integration_id")
        and integration.get("root_head_commit") == seal.get("final_commit"),
        "integration_invalid",
    )
    integrated_state = integration.get("root_workspace_state_after")
    agent_outcomes = [
        stored
        for stored in root
        if kind(stored) == "tool_outcome_committed"
        and stored.get("event", {}).get("name") == "agent"
    ]
    check(len(agent_outcomes) == 1, "integrated_result_invalid")
    agent_outcome_stored = agent_outcomes[0]
    agent_outcome = agent_outcome_stored["event"]
    finished = one(root, "child_finished").get("outcome", {}).get("details", {}).get("integration", {})
    if (
        agent_outcome.get("name") != "agent"
        or agent_outcome.get("workspace_state") != integrated_state
        or finished.get("state") != "integrated"
        or finished.get("root_workspace_state") != integrated_state
    ):
        raise Failure("integrated_result_invalid")

    root_terminal = one(root, "terminal")["outcome"]["terminal"]
    root_receipt = receipt(
        one(root, "host_verification_committed"),
        root_terminal,
        expected_acceptance,
        root_id,
    )
    check_post_integration_receipt(integrated_state, root_receipt.get("workspace_state"))
    cleanup_plan = one(root, "agent_cleanup_prepared").get("plan", {})
    cleanup = one(root, "agent_cleanup_committed").get("result", {})
    ownership = cleanup_plan.get("ownership", {})
    artifact = cleanup_plan.get("artifact_state", {})
    scope = cleanup_plan.get("scope", {})
    cleanup_revision = scope.get("workspace_revision", {})
    mode = cleanup_plan.get("mode", {})
    expected_path_digest = hashlib.sha256(
        json.dumps([FILE], ensure_ascii=False, separators=(",", ":")).encode()
    ).hexdigest()
    cleanup_result_valid = (
        cleanup.get("status") == "already_absent"
        or (
            cleanup.get("status") == "removed"
            and cleanup.get("worktree") in {"removed", "already_absent"}
            and cleanup.get("branch") in {"removed", "already_absent"}
            and not (
                cleanup.get("worktree") == "already_absent"
                and cleanup.get("branch") == "already_absent"
            )
        )
    )
    if (
        cleanup_plan.get("phase") != "post_integration"
        or cleanup_plan.get("reason_code") != "writer_integrated"
        or ownership.get("state") != "known"
        or not isinstance(ownership.get("identity_sha256"), str)
        or not re.fullmatch(r"[0-9a-f]{64}", ownership["identity_sha256"])
        or artifact
        != {
            "state": "known_host_sealed",
            "final_commit": seal.get("final_commit"),
            "diff_sha256": seal.get("diff_sha256"),
        }
        or scope.get("state") != "known"
        or cleanup_revision.get("status") != "known"
        or not isinstance(cleanup_revision.get("sha256"), str)
        or not re.fullmatch(r"[0-9a-f]{64}", cleanup_revision["sha256"])
        or scope.get("changed_count") != 1
        or scope.get("in_scope_count") != 1
        or scope.get("out_of_scope_count") != 0
        or scope.get("path_set_sha256") != expected_path_digest
        or mode
        != {
            "mode": "remove_exact",
            "expected_branch_commit": seal.get("final_commit"),
        }
        or not cleanup_result_valid
    ):
        raise Failure("root_receipt_or_cleanup_invalid")
    ordered(
        root,
        [
            "agent_task_prepared",
            "agent_workspace_created",
            "child_started",
            "agent_seal_prepared",
            "agent_seal_committed",
            "agent_result_collected",
            "agent_integration_prepared",
            "agent_integration_started",
            "agent_integration_committed",
            "child_finished",
            "host_verification_committed",
            "agent_cleanup_prepared",
            "agent_cleanup_committed",
            "terminal",
        ],
    )
    child_finished_position = next(
        index for index, stored in enumerate(root) if kind(stored) == "child_finished"
    )
    agent_outcome_position = root.index(agent_outcome_stored)
    host_verification_position = next(
        index
        for index, stored in enumerate(root)
        if kind(stored) == "host_verification_committed"
    )
    check(
        child_finished_position < agent_outcome_position < host_verification_position,
        "agent_outcome_order_invalid",
    )
    if kind(root[-1]) != "terminal" or kind(child[-1]) != "terminal":
        raise Failure("terminal_not_last")
    return {
        "agent_task_id": agent_task["task_id"],
        "child_run_id": child_id,
        "writer_base_commit": base,
        "writer_commit": seal["final_commit"],
        "root_branch_ref": assignment["root_branch"],
        "writer_branch": assignment["branch"],
        "worktree_path": assignment["worktree_path"],
        "writer_allowed_paths": assignment["allowed_paths"],
        "writer_changed_files": seal["changed_files"],
        "writer_diff_sha256": seal["diff_sha256"],
        "integration_id": integration["integration_id"],
        "integration_status": "integrated",
        "child_receipt_sha256": json_digest(child_receipt),
        "root_receipt_sha256": json_digest(root_receipt),
        "root_receipt_generation_id": root_receipt["generation_id"],
        "root_receipt_workspace_generation": root_receipt["workspace_state"]["generation"],
        "root_receipt_revision": root_receipt["workspace_state"]["revision"],
    }

def audit_git(workspace: Path, base: str, facts: dict[str, Any], state: Path) -> dict[str, Any]:
    head = git(workspace, "rev-parse", "HEAD")
    changed = git(workspace, "diff", "--name-only", "--no-renames", base, head).splitlines()
    worktrees = [
        line.removeprefix("worktree ")
        for line in git(workspace, "worktree", "list", "--porcelain").splitlines()
        if line.startswith("worktree ")
    ]
    managed = state / "codewhale" / "worktrees"
    if (
        (workspace / FILE).read_bytes() != AFTER
        or git(workspace, "status", "--porcelain=v1")
        or head != facts["writer_commit"]
        or git(workspace, "rev-list", "--count", f"{base}..{head}") != "1"
        or changed != [FILE]
        or worktrees != [str(workspace.resolve())]
        or git(workspace, "for-each-ref", "--format=%(refname)", "refs/heads/codewhale/writer/")
        or (managed.is_dir() and any(managed.iterdir()))
    ):
        raise Failure("git_integration_or_cleanup_invalid")
    ancestor = subprocess.run(
        ["git", "merge-base", "--is-ancestor", base, head],
        cwd=workspace,
        env=safe_env(),
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        check=False,
    )
    if ancestor.returncode:
        raise Failure("integration_not_fast_forward")
    return {
        "base_commit": base,
        "head_commit": head,
        "changed_files": changed,
        "final_file_sha256": digest(AFTER),
        "fast_forward": True,
        "root_clean": True,
        "worktree_removed": True,
        "branch_removed": True,
    }

def accounting(run: dict[str, Any]) -> dict[str, Any]:
    value = run.get("accounting", {})
    root = value.get("root", {})
    child = value.get("child", {})
    try:
        result: dict[str, Any] = {
            name: int(root[name]) + int(child[name])
            for name in ("started", "completed", "in_flight")
        }
        cost_nanousd = int(value["cost_nanousd"])
        cost_nanocny = int(value["cost_nanocny"])
    except (KeyError, TypeError, ValueError) as error:
        raise Failure("accounting_invalid") from error
    result["limit"] = value.get("hard_request_limit")
    result["transport_retries"] = value.get("transport_retries")
    result["cost_usd"] = cost_nanousd / 1_000_000_000
    result["cost_cny"] = cost_nanocny / 1_000_000_000
    result["surface_usage"] = value.get("surface_usage")
    terminal = run.get("terminal", {})
    terminal_state = terminal.get("state")
    if (
        terminal_state != "completed"
        or value.get("complete") is not True
        or value.get("usage_complete") is not True
        or value.get("usage_missing") is not False
        or value.get("usage_incomplete") is not False
        or value.get("billing_unknown") is not False
        or result["started"] < 1
        or result["started"] != result["completed"]
        or result["in_flight"] != 0
        or result["limit"] != MAX_REQUESTS
        or cost_nanousd < 0
        or cost_nanocny < 0
        or value.get("unpriced") is not False
        or not isinstance(result["surface_usage"], list)
        or not result["surface_usage"]
    ):
        surface_usage = result["surface_usage"]
        raise Failure(
            "accounting_invalid",
            {
                "terminal_state": terminal_state,
                "terminal_reason": " ".join(str(terminal.get("reason", "")).split())[:512],
                "runtime_model_requests": run.get("runtime_model_requests"),
                "runtime_retries": run.get("runtime_retries"),
                "tool_calls": run.get("tool_calls"),
                "local_turns": run.get("local_turns"),
                "root": root,
                "child": child,
                "hard_request_limit": result["limit"],
                "transport_retries": result["transport_retries"],
                "complete": value.get("complete"),
                "usage_complete": value.get("usage_complete"),
                "usage_missing": value.get("usage_missing"),
                "usage_incomplete": value.get("usage_incomplete"),
                "billing_unknown": value.get("billing_unknown"),
                "unpriced": value.get("unpriced"),
                "started": result["started"],
                "completed": result["completed"],
                "in_flight": result["in_flight"],
                "cost_nanousd": cost_nanousd,
                "cost_nanocny": cost_nanocny,
                "surface_usage": [
                    {
                        key: entry.get(key)
                        for key in (
                            "surface",
                            "model",
                            "response_count",
                            "usage_response_count",
                        )
                    }
                    for entry in surface_usage
                    if isinstance(entry, dict)
                ]
                if isinstance(surface_usage, list)
                else None,
            },
        )
    return result

def stop(
    process: subprocess.Popen[bytes],
    timeout_seconds: float = 5,
) -> None:
    if process.poll() is not None:
        return
    try:
        os.killpg(process.pid, signal.SIGTERM)
        process.wait(timeout=max(0.0, timeout_seconds))
    except (OSError, subprocess.TimeoutExpired):
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except OSError:
            pass

def live(args: argparse.Namespace, disclosure: dict[str, Any]) -> dict[str, Any]:
    if not args.acknowledge_cost:
        raise Failure("cost_acknowledgement_required")
    if not args.key_file:
        raise Failure("key_file_required")
    revision = (args.candidate_revision or "").strip()
    if not args.candidate_binary or len(revision) != 40 or any(c not in "0123456789abcdef" for c in revision):
        raise Failure("candidate_identity_required")

    started = time.monotonic()
    evaluation_id = str(uuid.uuid4())
    record: dict[str, Any]
    with tempfile.TemporaryDirectory(prefix="codewhale-m6-writer-") as raw:
        root = Path(raw)
        workspace, base = make_workspace(root)
        source = Path(args.candidate_binary).expanduser().resolve()
        if not source.is_file() or not os.access(source, os.X_OK):
            raise Failure("candidate_binary_unavailable")
        binary = root / "codewhale"
        shutil.copy2(source, binary)
        binary.chmod(0o700)
        binary_sha = digest(binary.read_bytes())
        disclosure.update(
            candidate_revision=revision,
            candidate_binary_sha256=binary_sha,
            model=args.model,
        )
        state = root / "state"
        home = state / "home"
        codewhale_home = state / "codewhale"
        xdg = state / "xdg"
        for directory in (home, codewhale_home, xdg):
            directory.mkdir(parents=True)
        credentialless_env = {
            **safe_env(),
            "HOME": str(home),
            "CODEWHALE_HOME": str(codewhale_home),
            "XDG_CONFIG_HOME": str(xdg),
        }
        binary_identity = probe_binary(binary, workspace, credentialless_env, revision)
        key = read_key(Path(args.key_file).expanduser())
        secret = key.encode()
        disclosure["key_accessed"] = True
        env = {**credentialless_env, "DEEPSEEK_API_KEY": key}
        check(not any(name in env for name in NETWORK_OVERRIDE_ENV), "network_override_present")
        key = ""
        stderr_path = state / "app-server.stderr"
        try:
            with stderr_path.open("wb") as stderr_stream:
                process = subprocess.Popen(
                    [str(binary), "--provider", "deepseek", "app-server", "--stdio"],
                    cwd=workspace,
                    env=env,
                    stdin=subprocess.PIPE,
                    stdout=subprocess.PIPE,
                    stderr=stderr_stream,
                    start_new_session=True,
                )
        except OSError as error:
            env["DEEPSEEK_API_KEY"] = ""
            raise Failure("app_server_launch_failed") from error
        env["DEEPSEEK_API_KEY"] = ""
        disclosure["paid_request_started"] = None
        client: Stdio | None = None
        failure: Failure | None = None
        try:
            client = Stdio(process, secret)
            expected_task = task(Path(sys.executable))
            result = client.call(
                start_command(workspace, Path(sys.executable), args.model, f"m6-start-{evaluation_id}")
            )
            if result.get("kind") != "run":
                raise Failure("start_run_missing")
            disclosure["paid_request_started"] = True
            run = result["run"]
            check(run.get("model") == args.model, "root_model_projection_invalid")
            root_id = run.get("run_id")
            if not isinstance(root_id, str):
                raise Failure("root_run_id_missing")
            deadline = time.monotonic() + HARNESS_SECONDS
            poll = 0
            while run.get("terminal") is None:
                poll += 1
                returncode = process.poll()
                if returncode is not None:
                    stderr = stderr_path.read_bytes()
                    if secret in stderr:
                        raise Failure("key_in_stderr")
                    raise Failure(
                        "app_server_exited",
                        {
                            "process_returncode": returncode,
                            "stderr_sha256": digest(stderr),
                            "stderr_tail": stderr[-8192:].decode(errors="replace"),
                            "poll_count": poll,
                            "wall_time_ms": int((time.monotonic() - started) * 1000),
                        },
                    )
                if time.monotonic() >= deadline:
                    try:
                        progress = collect_progress(client, root_id, run, "deadline")
                    except Failure as progress_error:
                        progress = {"collection_error": progress_error.code}
                    raise Failure(
                        "run_deadline_exceeded",
                        {
                            "poll_count": poll,
                            "wall_time_ms": int((time.monotonic() - started) * 1000),
                            "progress": progress,
                        },
                    )
                if poll > MAX_POLLS:
                    try:
                        progress = collect_progress(client, root_id, run, "poll-limit")
                    except Failure as progress_error:
                        progress = {"collection_error": progress_error.code}
                    raise Failure(
                        "run_poll_limit_exceeded",
                        {
                            "poll_count": poll,
                            "wall_time_ms": int((time.monotonic() - started) * 1000),
                            "progress": progress,
                        },
                    )
                time.sleep(0.2)
                result = client.call(query("get", root_id, f"m6-get-{poll}"))
                if result.get("kind") != "run":
                    raise Failure("run_view_missing")
                run = result["run"]
                check(run.get("model") == args.model, "root_model_projection_invalid")
            try:
                request_counts = accounting(run)
            except Failure as error:
                try:
                    progress = collect_progress(client, root_id, run, "terminal-failure")
                except Failure as progress_error:
                    progress = {"collection_error": progress_error.code}
                raise Failure(
                    error.code,
                    {
                        **error.details,
                        "progress": progress,
                    },
                ) from error
            disclosure["paid_request_started"] = True
            root_events = events(client, root_id, "m6-root-events")
            child_id = one(root_events, "agent_task_prepared")["task"]["child_run_id"]
            child_result = client.call(query("get", child_id, "m6-child-get"))
            if (
                child_result.get("kind") != "run"
                or child_result["run"].get("terminal") is None
                or child_result["run"].get("model") != args.model
            ):
                raise Failure("child_run_missing")
            child_events = events(client, child_id, "m6-child-events")
            facts = audit(root_events, child_events, expected_task, root_id, base, args.model)
            git_facts = audit_git(workspace, base, facts, state)
            usage = run.get("usage", {})
            tokens = {
                name: usage.get(field)
                for name, field in {
                    "input": "input_tokens", "output": "output_tokens",
                    "cache_hit": "cache_hit_tokens", "cache_miss": "cache_miss_tokens",
                    "reasoning": "reasoning_tokens",
                }.items()
            }
            record = {
                "schema": SCHEMA, "record_type": "canary_result", "status": "passed",
                "record_class": "mechanism_canary",
                "product_metric_eligible": False, "evaluation_id": evaluation_id,
                "candidate_revision": revision, "candidate_binary_sha256": binary_sha,
                "binary_identity": {
                    **binary_identity, "build_revision_bound": True,
                    "version_probe_argv": ["--version"],
                    "runtime_argv": ["--provider", "deepseek", "app-server", "--stdio"],
                    "version_probe_without_credentials": True,
                    "proxy_or_custom_ca_inherited": False,
                },
                "model": args.model, "run_api_schema_version": RUN_API,
                "runtime_event_schema_version": EVENT_API, "requests": request_counts,
                "task_definition_sha256": json_digest(expected_task),
                "verifier_spec_sha256": json_digest(verifier(Path(sys.executable))),
                "tokens": tokens, "git": git_facts,
                "writer": {
                    name: facts[name]
                    for name in (
                        "agent_task_id",
                        "child_run_id",
                        "writer_base_commit",
                        "writer_commit",
                        "root_branch_ref",
                        "writer_branch",
                        "worktree_path",
                        "writer_allowed_paths",
                        "writer_changed_files",
                        "writer_diff_sha256",
                        "integration_id",
                        "integration_status",
                    )
                },
                "child_receipt_sha256": facts["child_receipt_sha256"],
                "root_receipt_sha256": facts["root_receipt_sha256"],
                "root_receipt": {
                    "generation_id": facts["root_receipt_generation_id"],
                    "workspace_generation": facts["root_receipt_workspace_generation"],
                    "revision": facts["root_receipt_revision"],
                },
                "wall_time_ms": int((time.monotonic() - started) * 1000),
                "key_accessed": True,
                "note": "单次 M6 机制 canary，不具备产品指标资格",
            }
        except Failure as error:
            failure = error
        finally:
            if client is not None:
                client.close()
            stop(process)
        runtime_argv = ["--provider", "deepseek", "app-server", "--stdio"]
        check(not any(secret in value.encode() for value in runtime_argv), "key_in_argv")
        check(not tree_contains(workspace, secret), "key_in_git")
        check(not tree_contains(state, secret), "key_in_local_state")
        if failure is not None:
            raise failure
        record.update(
            key_in_argv=False,
            key_in_protocol=False,
            key_in_git=False,
            key_in_local_state=False,
            secret_scans=["stdio_frames", "fixture_git", "ephemeral_local_state"],
        )
        secret = b""
    check(not root.exists(), "ephemeral_state_not_deleted")
    record["ephemeral_state_deleted"] = True
    return record

def self_test() -> None:
    def rejects(code: str, action: Any) -> None:
        try:
            action()
        except Failure as error:
            assert error.code == code
        else:
            raise AssertionError(f"{code} was accepted")

    spec = task(Path(sys.executable))
    assert len(spec["acceptance"]) == 1 and spec["acceptance"][0]["kind"] == "verifier"
    envelope = start_command(Path("/tmp/workspace"), Path(sys.executable), MODEL, "test")
    assert (
        envelope["schema_version"] == RUN_API
        and envelope["command"]["max_api_requests"] == MAX_REQUESTS
        and envelope["command"]["tool_policy"]["allowed"] == ROOT_TOOLS
        and MAX_POLLS * 0.2 >= HARNESS_SECONDS
        and not any(name in safe_env() for name in NETWORK_OVERRIDE_ENV)
    )
    revision = "a" * 40
    assert parse_binary_version(b"codewhale 0.8.68 (aaaaaaaaaaaa)\n", revision)["revision_prefix"] == "a" * 12
    for invalid in (
        b"not-codewhale 0.8.68 (aaaaaaaaaaaa)\n",
        b"codewhale 0.8.68 (bbbbbbbbbbbb)\n",
    ):
        rejects("candidate_version_invalid", lambda value=invalid: parse_binary_version(value, revision))
    rejects("stdio_frame_invalid", lambda: bounded_line(io.BytesIO(b"unterminated")))
    rejects("stdio_frame_invalid", lambda: bounded_line(io.BytesIO(b"x" * (MAX_STDIO_FRAME + 1))))
    partial = subprocess.Popen(
        [
            sys.executable,
            "-c",
            "import os,time; os.write(1,b'{\"partial\":'); time.sleep(1)",
        ],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        start_new_session=True,
    )
    partial_client = Stdio(partial, b"forbidden")
    try:
        rejects(
            "stdio_timeout",
            lambda: partial_client.call(
                {"request_id": "partial-frame"},
                timeout_seconds=0.05,
            ),
        )
    finally:
        partial_client.close()
        stop(partial, timeout_seconds=0)
    echo = subprocess.Popen(
        [
            sys.executable,
            "-c",
            (
                "import json,sys; request=json.loads(sys.stdin.readline()); "
                f"print(json.dumps({{'schema_version':{RUN_API},"
                "'request_id':request['request_id'],"
                "'result':{'kind':'accepted','run_id':'run-1'}}),flush=True)"
            ),
        ],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        start_new_session=True,
    )
    echo_client = Stdio(echo, b"forbidden")
    try:
        assert echo_client.call(
            {"request_id": "complete-frame"},
            timeout_seconds=1,
        ) == {"kind": "accepted", "run_id": "run-1"}
    finally:
        echo_client.close()
        stop(echo, timeout_seconds=0)
    integrated = {"generation": 7, "revision": {"status": "known", "sha256": "sha256:x"}}
    check_post_integration_receipt(integrated, {"generation": 8, "revision": integrated["revision"]})
    rejects("root_receipt_not_latest", lambda: check_post_integration_receipt(integrated, integrated))
    rejects(
        "root_receipt_not_latest",
        lambda: check_post_integration_receipt(integrated, {"generation": 8, "revision": {"status": "known", "sha256": "sha256:y"}}),
    )
    identity = {
        "run_id": "root", "parent_run_id": None, "model": MODEL,
        "reasoning_effort": "high", "max_output_tokens": 8192, "streaming": True,
        "environment": {"provider": "deepseek"}, "actor": {"kind": "root", "depth": 0},
    }
    model_request = {key: value for key, value in identity.items() if key != "environment"}
    stream = [{"event": {"kind": "model_request_prepared", "request": model_request}}]
    audit_run_identity(stream, identity, "root", None, MODEL, True, "root", 0)
    summary = event_summary(
        [
            {"sequence": 1, "event": {"kind": "run_created"}},
            {"sequence": 2, "event": {"kind": "model_request_prepared"}},
            {"sequence": 3, "event": {"kind": "model_request_prepared"}},
        ]
    )
    assert summary == {
        "last_sequence": 3,
        "event_counts": {"run_created": 1, "model_request_prepared": 2},
        "event_tail": [
            "run_created",
            "model_request_prepared",
            "model_request_prepared",
        ],
    }
    failed_summary = event_summary(
        [
            {
                "sequence": 1,
                "event": {
                    "kind": "model_request_failed",
                    "failure": {
                        "code": "stream_stall",
                        "category": "stream_stall",
                        "message": "must not be copied",
                        "retryable": True,
                        "actionable_output": True,
                    },
                    "retry": {
                        "decision": "stop",
                        "reason": "actionable_output",
                    },
                },
            }
        ]
    )
    assert failed_summary["model_failures"] == [
        {
            "sequence": 1,
            "code": "stream_stall",
            "category": "stream_stall",
            "message": "must not be copied",
            "retryable": True,
            "actionable_output": True,
            "retry_decision": "stop",
            "stop_reason": "actionable_output",
        }
    ]
    rejects(
        "run_identity_invalid",
        lambda: audit_run_identity(
            stream, {**identity, "streaming": False}, "root", None, MODEL, True, "root", 0
        ),
    )
    with tempfile.TemporaryDirectory(prefix="codewhale-m6-self-test-") as raw:
        root = Path(raw)
        workspace, _ = make_workspace(root)
        assert git(workspace, "symbolic-ref", "-q", "HEAD") == ROOT_BRANCH_REF
        command = [sys.executable, "-I", "-c", VERIFY_CODE]
        assert subprocess.run(command, cwd=workspace, check=False).returncode != 0
        (workspace / FILE).write_bytes(AFTER)
        assert subprocess.run(command, cwd=workspace, check=False).returncode == 0
        key_path = root / "key"
        key_path.write_text("test-only-key\n")
        key_path.chmod(0o600)
        assert read_key(key_path) == "test-only-key"
        key_path.chmod(0o644)
        rejects("key_mode_not_0600", lambda: read_key(key_path))

def emit(record: dict[str, Any], output: str | None) -> None:
    data = (
        json.dumps(record, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
        + "\n"
    ).encode()
    if output is None:
        sys.stdout.buffer.write(data)
        return
    path = Path(output).expanduser().resolve()
    if not path.parent.is_dir():
        raise Failure("output_parent_unavailable")
    with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as stream:
        temporary = Path(stream.name)
        temporary.chmod(0o600)
        stream.write(data)
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temporary, path)

def dry_run(args: argparse.Namespace) -> dict[str, Any]:
    expected_task = task(Path(sys.executable))
    chain = [
        "root agent", "isolated worktree", "child receipt", "seal",
        "fast-forward integration", "root latest receipt", "cleanup", "terminal",
    ]
    return {
        "schema": SCHEMA, "record_type": "dry_run", "status": "ready",
        "product_metric_eligible": False, "run_api_schema_version": RUN_API,
        "runtime_event_schema_version": EVENT_API, "model": args.model,
        "max_api_requests": MAX_REQUESTS, "max_runtime_seconds": RUNTIME_SECONDS,
        "task_definition_sha256": json_digest(expected_task),
        "verifier_spec_sha256": json_digest(verifier(Path(sys.executable))),
        "root_allowed_tools": ROOT_TOOLS, "writer_allowed_tools": WRITER_TOOLS,
        "expected_chain": chain, "key_accessed": False, "paid_request_started": False,
        "candidate_requirement": "codewhale <semver> (<candidate revision 前12位>)",
        "version_probe_argv": ["--version"],
        "root_branch_ref": ROOT_BRANCH_REF,
        "max_poll_count": MAX_POLLS,
        "max_stdio_frame_bytes": MAX_STDIO_FRAME,
        "proxy_or_custom_ca_inherited": False,
        "note": "dry-run 不读 Key、不启动二进制、不访问 DeepSeek",
    }

def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--acknowledge-cost", action="store_true")
    parser.add_argument("--key-file")
    parser.add_argument("--candidate-binary")
    parser.add_argument("--candidate-revision")
    parser.add_argument("--model", choices=MODELS, default=MODEL)
    parser.add_argument("--output")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        print("M6 writer canary self-test: PASS")
        return 0
    disclosure: dict[str, Any] = {"key_accessed": False, "paid_request_started": False}
    try:
        record = dry_run(args) if args.dry_run else live(args, disclosure)
    except Failure as error:
        record = {
            "schema": SCHEMA, "record_type": "canary_result", "status": "failed",
            "product_metric_eligible": False,
            "error_code": error.code,
            "error_details": error.details,
            **disclosure,
        }
        emit(record, args.output)
        return 1
    emit(record, args.output)
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
