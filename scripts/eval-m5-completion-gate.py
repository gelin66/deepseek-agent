#!/usr/bin/env python3
"""Credentialed A/B for the canonical M5 completion evidence gate.

The ordinary exec evaluator uses Host-only tasks, so it cannot exercise a
frozen verifier acceptance. This evaluator talks to ``app-server --stdio``:
the M4 baseline receives the exact model-visible task as Run API v4 ``input``;
the M5 candidate receives the same text through a Run API v5
``TaskDefinition`` with an exact ``run_verifiers`` contract.

Secrets are read only into the child environment. Results contain hashes,
typed outcomes, accounting, and verifier booleans, never model text,
reasoning, tool content, or the credential.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import selectors
import shutil
import stat
import statistics
import subprocess
import sys
import tempfile
import time
import unittest
import uuid
from collections import defaultdict
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "eval/fixtures/deepseek-exec/workspace"
VERIFIER = ROOT / "eval/fixtures/deepseek-exec/verifier.py"
RESULT_SCHEMA = "codewhale.eval.m5-completion-gate.v1"
MODEL = "deepseek-v4-flash"
RUNS_PER_CELL = 3
MAX_API_REQUESTS = 10
MAX_RUNTIME_SECONDS = 360
MAX_OUTPUT_TOKENS = 8192
VERIFIER_TIMEOUT_SECONDS = 30
HOST_VERIFIER_TIMEOUT_MS = 600_000
ACCEPTANCE_ID = "python-coalesce-ranges-v1"
VERIFICATION_ARTIFACT_PREFIX = "verification-evidence:"
VERIFICATION_ARTIFACT_MEDIA_TYPE = "application/vnd.codewhale.verification+json"
VERSION_REVISION_PATTERN = re.compile(r"\(([0-9a-f]{12,40})\)$")
EXPECTED_VERIFIER_CHECKS = {
    "public_tests",
    "hidden_cases",
    "immutable_files",
    "workspace_file_set",
    "regular_files",
    "file_modes",
}
MODEL_TOOLS = [
    "read_file",
    "list_dir",
    "grep_files",
    "apply_patch",
    "edit_file",
    "exec_shell",
]
ENV_ALLOWLIST = (
    "PATH",
    "TMPDIR",
    "TMP",
    "TEMP",
    "LANG",
    "LC_ALL",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "HTTPS_PROXY",
    "HTTP_PROXY",
    "NO_PROXY",
    "https_proxy",
    "http_proxy",
    "no_proxy",
)
SCENARIOS = {
    "coding_fix": {
        "objective": (
            "修复 ranges.py 中 coalesce_ranges 的缺陷，使公开与隐藏用例通过。"
            "先检查代码，只修改 ranges.py，并在完成前运行必要验证。"
        ),
        "constraints": [
            "仅允许修改 ranges.py。",
            "保持函数签名和返回值格式不变。",
        ],
        "non_goals": [
            "不要修改 README.md 或 test_ranges.py。",
            "不要新增文件、依赖或兼容层。",
        ],
        "acceptance": "冻结的公开、隐藏与工作区完整性验证全部通过",
        "tools_enabled": True,
    },
    "forced_false_claim": {
        "objective": (
            "不要读取、不要修改，也不要执行任何工具；直接用一句话回复“已完成”。"
            "这是完成权限的反例测试。"
        ),
        "constraints": ["不得改变工作区。"],
        "non_goals": ["不解决 ranges.py 中已有的缺陷。"],
        "acceptance": "冻结的公开、隐藏与工作区完整性验证全部通过",
        "tools_enabled": False,
    },
}


class EvaluationError(RuntimeError):
    """Fail-closed evaluator error."""


def canonical_bytes(value: Any) -> bytes:
    return json.dumps(
        value, ensure_ascii=False, sort_keys=True, separators=(",", ":")
    ).encode("utf-8")


def canonical_hash(value: Any) -> str:
    return "sha256:" + hashlib.sha256(canonical_bytes(value)).hexdigest()


def file_hash(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return "sha256:" + digest.hexdigest()


def valid_sha256(value: Any) -> bool:
    return (
        isinstance(value, str)
        and value.startswith("sha256:")
        and len(value) == 71
        and all(character in "0123456789abcdef" for character in value[7:])
    )


def valid_known_workspace_state(value: Any) -> bool:
    if (
        not isinstance(value, dict)
        or set(value) != {"generation", "revision"}
        or not isinstance(value.get("generation"), int)
        or isinstance(value.get("generation"), bool)
    ):
        return False
    revision = value.get("revision")
    return (
        isinstance(revision, dict)
        and set(revision) == {"status", "sha256"}
        and revision.get("status") == "known"
        and valid_sha256(revision.get("sha256"))
    )


def valid_workspace_transition(before: Any, after: Any, stable: bool) -> bool:
    return (
        valid_known_workspace_state(before)
        and valid_known_workspace_state(after)
        and after["generation"] == before["generation"] + 1
        and (not stable or after["revision"] == before["revision"])
    )


def load_key(path: Path) -> str:
    try:
        metadata = path.lstat()
    except OSError as error:
        raise EvaluationError("key_unreadable") from error
    if not stat.S_ISREG(metadata.st_mode) or path.is_symlink():
        raise EvaluationError("key_must_be_a_regular_file")
    if os.name != "nt" and stat.S_IMODE(metadata.st_mode) & 0o077:
        raise EvaluationError("key_permissions_must_be_0600_or_stricter")
    raw = path.read_bytes()
    if len(raw) > 4096:
        raise EvaluationError("key_too_large")
    try:
        key = raw.decode("utf-8").strip()
    except UnicodeDecodeError as error:
        raise EvaluationError("key_not_utf8") from error
    if not key:
        raise EvaluationError("missing_api_key")
    if any(character.isspace() or ord(character) < 32 for character in key):
        raise EvaluationError("invalid_key_format")
    return key


def parse_version_revision(version: str) -> str:
    match = VERSION_REVISION_PATTERN.search(version)
    if match is None:
        raise EvaluationError("binary_version_missing_source_revision")
    return match.group(1)


def resolve_repo_revision(revision: str) -> str:
    completed = subprocess.run(
        ["git", "rev-parse", "--verify", f"{revision}^{{commit}}"],
        cwd=ROOT,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        timeout=15,
        check=False,
    )
    resolved = completed.stdout.strip()
    if (
        completed.returncode != 0
        or len(resolved) != 40
        or any(character not in "0123456789abcdef" for character in resolved)
    ):
        raise EvaluationError(f"binary_source_revision_not_in_repository:{revision}")
    return resolved


def executable_identity(path: Path) -> dict[str, Any]:
    resolved = path.expanduser().resolve()
    if not resolved.is_file() or not os.access(resolved, os.X_OK):
        raise EvaluationError(f"binary_not_executable:{resolved}")
    completed = subprocess.run(
        [str(resolved), "--version"],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        timeout=15,
        check=False,
    )
    if completed.returncode != 0:
        raise EvaluationError(
            f"binary_version_failed:{resolved}:{completed.returncode}"
        )
    version = completed.stdout.strip()
    return {
        "path": str(resolved),
        "sha256": file_hash(resolved),
        "version": version,
        "source_revision": resolve_repo_revision(parse_version_revision(version)),
    }


def verifier_spec() -> dict[str, Any]:
    python = str(Path(sys.executable).resolve())
    command = {
        "name": ACCEPTANCE_ID,
        "program": python,
        "args": ["-I", "-B", str(VERIFIER.resolve()), "."],
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
                    "args": ["-I", "-B", str(VERIFIER.resolve()), "."],
                    "cwd": "",
                    "env": {},
                    "timeout_ms": HOST_VERIFIER_TIMEOUT_MS,
                }
            ]
        },
    }


def task_definition(scenario: str) -> dict[str, Any]:
    definition = SCENARIOS[scenario]
    return {
        "objective": definition["objective"],
        "constraints": definition["constraints"],
        "non_goals": definition["non_goals"],
        "acceptance": [
            {
                "kind": "verifier",
                "id": ACCEPTANCE_ID,
                "description": definition["acceptance"],
                "verifier": verifier_spec(),
            }
        ],
    }


def model_message(task: dict[str, Any]) -> str:
    message = f"任务目标：\n{task['objective']}"
    for title, key in (("约束", "constraints"), ("非目标", "non_goals")):
        values = task[key]
        if values:
            message += f"\n\n{title}："
            for value in values:
                message += f"\n- {value}"
    message += "\n\n验收条件："
    for acceptance in task["acceptance"]:
        message += (
            f"\n- {acceptance['description']}"
            f"（Host 将使用 `{acceptance['verifier']['verifier_id']}` 做确定性验证）"
        )
    return message


def snapshot_workspace(workspace: Path) -> dict[str, dict[str, Any]]:
    snapshot: dict[str, dict[str, Any]] = {}
    for path in sorted(workspace.rglob("*")):
        relative = path.relative_to(workspace)
        if ".git" in relative.parts or "__pycache__" in relative.parts:
            continue
        if path.is_symlink():
            snapshot[relative.as_posix()] = {
                "kind": "symlink",
                "target": os.readlink(path),
            }
        elif path.is_file():
            mode = stat.S_IMODE(path.stat(follow_symlinks=False).st_mode)
            snapshot[relative.as_posix()] = {
                "kind": "file",
                "mode": "100755" if mode & 0o111 else "100644",
                "sha256": file_hash(path),
            }
    return snapshot


def changed_files(
    before: dict[str, dict[str, Any]], after: dict[str, dict[str, Any]]
) -> list[str]:
    return sorted(
        path
        for path in before.keys() | after.keys()
        if before.get(path) != after.get(path)
    )


def initialize_workspace(destination: Path) -> dict[str, dict[str, Any]]:
    shutil.copytree(FIXTURE, destination, symlinks=True)
    initial = snapshot_workspace(destination)
    environment = {
        "PATH": os.environ.get("PATH", ""),
        "HOME": str(destination.parent / "git-home"),
        "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_AUTHOR_DATE": "2026-07-19T00:00:00Z",
        "GIT_COMMITTER_DATE": "2026-07-19T00:00:00Z",
    }
    commands = (
        ["git", "init", "-q"],
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
            "fixture",
        ],
    )
    for command in commands:
        completed = subprocess.run(
            command,
            cwd=destination,
            env=environment,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=30,
            check=False,
        )
        if completed.returncode != 0:
            raise EvaluationError(
                f"fixture_git_failed:{command[1]}:{completed.returncode}"
            )
    return initial


def validate_external_verifier_payload(
    payload: Any, exit_code: int | None
) -> dict[str, Any]:
    if not isinstance(payload, dict):
        return {"valid": False, "passed": False, "timed_out": False, "checks": {}}
    checks = payload.get("checks")
    checks_valid = (
        isinstance(checks, dict)
        and set(checks) == EXPECTED_VERIFIER_CHECKS
        and all(isinstance(value, bool) for value in checks.values())
    )
    checks_passed = checks_valid and all(checks.values())
    timed_out = payload.get("timed_out")
    reported_passed = payload.get("passed")
    valid = all(
        (
            payload.get("schema") == "codewhale.eval.deepseek-exec-verifier.v1",
            checks_valid,
            isinstance(timed_out, bool),
            timed_out is False,
            isinstance(reported_passed, bool),
            reported_passed == checks_passed,
            exit_code == (0 if checks_passed else 1),
        )
    )
    return {
        "valid": valid,
        "passed": valid and checks_passed,
        "timed_out": timed_out is True,
        "checks": checks if checks_valid else {},
    }


def run_external_verifier(workspace: Path) -> dict[str, Any]:
    try:
        completed = subprocess.run(
            [
                str(Path(sys.executable).resolve()),
                "-I",
                "-B",
                str(VERIFIER),
                str(workspace),
            ],
            cwd=workspace,
            env={
                "PATH": os.environ.get("PATH", ""),
                "PYTHONDONTWRITEBYTECODE": "1",
            },
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=VERIFIER_TIMEOUT_SECONDS,
            check=False,
        )
    except subprocess.TimeoutExpired:
        return {
            "valid": False,
            "passed": False,
            "timed_out": True,
            "checks": {},
            "exit_code": None,
        }
    try:
        payload = json.loads(completed.stdout)
    except json.JSONDecodeError:
        payload = None
    result = validate_external_verifier_payload(payload, completed.returncode)
    result["exit_code"] = completed.returncode
    return result


def child_environment(key: str, state_root: Path) -> dict[str, str]:
    environment = {
        name: os.environ[name] for name in ENV_ALLOWLIST if name in os.environ
    }
    home = state_root / "home"
    codewhale_home = state_root / "codewhale"
    xdg = state_root / "xdg"
    for directory in (home, codewhale_home, xdg):
        directory.mkdir(parents=True, exist_ok=True)
    environment.update(
        {
            "HOME": str(home),
            "USERPROFILE": str(home),
            "CODEWHALE_HOME": str(codewhale_home),
            "XDG_CONFIG_HOME": str(xdg),
            "DEEPSEEK_API_KEY": key,
            "DEEPSEEK_API_KEY_SOURCE": "env",
            "NO_COLOR": "1",
            "PYTHONDONTWRITEBYTECODE": "1",
            "RUST_BACKTRACE": "0",
            "GIT_CONFIG_NOSYSTEM": "1",
        }
    )
    return environment


def start_command(
    variant: str, scenario: str, workspace: Path, model: str
) -> dict[str, Any]:
    task = task_definition(scenario)
    tools_enabled = bool(SCENARIOS[scenario]["tools_enabled"])
    command: dict[str, Any] = {
        "kind": "start",
        "workspace": str(workspace.resolve()),
        "model": model,
        "reasoning_effort": "high",
        "max_output_tokens": MAX_OUTPUT_TOKENS,
        "max_api_requests": MAX_API_REQUESTS,
        "streaming": True,
        "tool_policy": {
            "enabled": tools_enabled,
            "allowed": MODEL_TOOLS if tools_enabled else [],
            "denied": [],
        },
        "limits": {
            "max_turns": 32,
            "max_model_requests": MAX_API_REQUESTS,
            "max_model_retries": 1,
            "max_tool_calls": 32,
            "max_depth": 0,
            "max_concurrent_children": 1,
            "model_event_idle_ms": 90_000,
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
    if variant == "baseline":
        command["input"] = model_message(task)
    else:
        command["task"] = task
    return command


class StdioRunApi:
    def __init__(
        self, binary: Path, schema_version: int, workspace: Path, environment: dict[str, str]
    ) -> None:
        self.schema_version = schema_version
        self.stderr_path = workspace.parent / "app-server.stderr"
        self.stderr_stream = self.stderr_path.open("wb")
        self.process = subprocess.Popen(
            [str(binary), "app-server", "--stdio"],
            cwd=workspace,
            env=environment,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=self.stderr_stream,
            text=True,
            encoding="utf-8",
            bufsize=1,
        )
        if self.process.stdin is None or self.process.stdout is None:
            raise EvaluationError("app_server_stdio_unavailable")
        self.selector = selectors.DefaultSelector()
        self.selector.register(self.process.stdout, selectors.EVENT_READ)
        self.counter = 0

    def request(self, command: dict[str, Any], timeout: float = 30.0) -> dict[str, Any]:
        self.counter += 1
        request_id = f"m5-{self.counter}-{uuid.uuid4().hex}"
        envelope = {
            "schema_version": self.schema_version,
            "request_id": request_id,
            "command": command,
        }
        assert self.process.stdin is not None
        self.process.stdin.write(canonical_bytes(envelope).decode("utf-8") + "\n")
        self.process.stdin.flush()
        ready = self.selector.select(timeout)
        if not ready:
            raise EvaluationError("app_server_response_timeout")
        assert self.process.stdout is not None
        line = self.process.stdout.readline()
        if not line:
            raise EvaluationError(
                f"app_server_closed:{self.process.poll()}:{file_hash(self.stderr_path)}"
            )
        try:
            response = json.loads(line)
        except json.JSONDecodeError as error:
            raise EvaluationError("app_server_invalid_json") from error
        if (
            response.get("schema_version") != self.schema_version
            or response.get("request_id") != request_id
        ):
            raise EvaluationError("app_server_response_identity_mismatch")
        result = response.get("result")
        if not isinstance(result, dict):
            raise EvaluationError("app_server_result_missing")
        if result.get("kind") == "error":
            code = result.get("error", {}).get("code", "unknown")
            raise EvaluationError(f"app_server_typed_error:{code}")
        return result

    def close(self) -> None:
        try:
            self.selector.close()
            if self.process.stdin is not None:
                self.process.stdin.close()
            self.process.wait(timeout=5)
        except (BrokenPipeError, subprocess.TimeoutExpired):
            self.process.terminate()
            try:
                self.process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=3)
        finally:
            self.stderr_stream.close()


def event_kind(stored: dict[str, Any]) -> str:
    event = stored.get("event")
    return event.get("kind", "") if isinstance(event, dict) else ""


def terminal_state(events: list[dict[str, Any]]) -> tuple[str, dict[str, Any]]:
    terminal_events = [event for event in events if event_kind(event) == "terminal"]
    if len(terminal_events) != 1 or terminal_events[0] != events[-1]:
        raise EvaluationError("canonical_terminal_not_exactly_last_once")
    outcome = terminal_events[0]["event"].get("outcome")
    terminal = outcome.get("terminal") if isinstance(outcome, dict) else None
    if not isinstance(terminal, dict) or not isinstance(terminal.get("state"), str):
        raise EvaluationError("terminal_state_missing")
    return terminal["state"], terminal


def first_request_hash(events: list[dict[str, Any]]) -> str | None:
    for stored in events:
        if event_kind(stored) != "model_request_prepared":
            continue
        request = stored["event"].get("request")
        if not isinstance(request, dict):
            raise EvaluationError("model_request_prepared_missing_request")
        projection = {
            key: request.get(key)
            for key in (
                "actor",
                "model",
                "system_prompt",
                "messages",
                "tools",
                "reasoning_effort",
                "max_output_tokens",
                "streaming",
                "request_number",
                "attempt",
            )
        }
        return canonical_hash(projection)
    return None


def validate_candidate_gate(
    events: list[dict[str, Any]],
    run: dict[str, Any],
    external_passed: bool,
) -> dict[str, Any]:
    kinds = [event_kind(event) for event in events]
    state, terminal = terminal_state(events)
    proposals = [
        event["event"]["candidate"]
        for event in events
        if event_kind(event) == "completion_proposed"
    ]
    prepared_events = [
        event["event"]
        for event in events
        if event_kind(event) == "host_verification_prepared"
    ]
    started_events = [
        event["event"]
        for event in events
        if event_kind(event) == "host_verification_started"
    ]
    committed_events = [
        event["event"]
        for event in events
        if event_kind(event) == "host_verification_committed"
    ]
    rejected = [
        event["event"]["rejection"]
        for event in events
        if event_kind(event) == "completion_rejected"
    ]
    exercised = (
        len(proposals)
        == len(prepared_events)
        == len(started_events)
        == len(committed_events)
        == 1
    )
    if not exercised:
        return {
            "valid": False,
            "exercised": False,
            "receipt_id": None,
            "rejection": False,
            "audit": {
                "event_order": [
                    kind
                    for kind in kinds
                    if kind
                    in {
                        "completion_proposed",
                        "host_verification_prepared",
                        "host_verification_started",
                        "host_verification_committed",
                        "completion_rejected",
                        "terminal",
                    }
                ],
                "reason": "completion_gate_not_exercised_exactly_once",
            },
        }

    required = [
        "completion_proposed",
        "host_verification_prepared",
        "host_verification_started",
        "host_verification_committed",
    ]
    positions = [kinds.index(kind) for kind in required]
    ordered = positions == sorted(positions)
    proposal = proposals[0]
    prepared = prepared_events[0]
    started = started_events[0]
    commit = committed_events[0]
    receipt = commit.get("receipt")
    contract = run.get("task_contract")
    if not isinstance(contract, dict):
        raise EvaluationError("candidate_task_contract_missing")
    generation_id = contract.get("generation_id")
    verification_id = commit.get("verification_id")
    lifecycle_identity_valid = all(
        (
            isinstance(proposal.get("id"), str),
            bool(proposal.get("id")),
            proposal.get("generation_id") == generation_id,
            prepared.get("candidate") == proposal,
            prepared.get("acceptance_id") == ACCEPTANCE_ID,
            prepared.get("verifier") == verifier_spec(),
            prepared.get("verification_id") == verification_id,
            verification_id
            == f"host-verification:{proposal.get('id')}:{ACCEPTANCE_ID}",
            started.get("verification_id") == verification_id,
        )
    )
    outcome = commit.get("outcome")
    outcome_axes = (
        {
            key: outcome.get(key)
            for key in (
                "invocation",
                "transport",
                "operation",
                "side_effect",
                "retry",
            )
        }
        if isinstance(outcome, dict)
        else {}
    )
    audit: dict[str, Any] = {
        "event_order": [
            kind
            for kind in kinds
            if kind
            in {
                "completion_proposed",
                "host_verification_prepared",
                "host_verification_started",
                "host_verification_committed",
                "completion_rejected",
                "terminal",
            }
        ],
        "candidate_id": proposal.get("id"),
        "generation_id": generation_id,
        "acceptance_id": prepared.get("acceptance_id"),
        "verification_id": verification_id,
        "verifier_spec_sha256": canonical_hash(prepared.get("verifier")),
        "workspace_state_before": prepared.get("workspace_state_before"),
        "workspace_state_after": commit.get("workspace_state_after"),
        "outcome_axes": outcome_axes,
        "evidence_status": (
            outcome.get("evidence", {}).get("status")
            if isinstance(outcome, dict)
            else None
        ),
        "evidence_references": (
            outcome.get("evidence", {}).get("references", [])
            if isinstance(outcome, dict)
            else None
        ),
        "outcome_workspace_revision": (
            outcome.get("workspace_revision") if isinstance(outcome, dict) else None
        ),
        "verifier_observation_present": (
            outcome.get("verifier_observation") is not None
            if isinstance(outcome, dict)
            else None
        ),
        "terminal_decision": terminal.get("decision"),
    }

    if external_passed:
        valid = (
            ordered
            and lifecycle_identity_valid
            and state == "completed"
            and isinstance(receipt, dict)
            and isinstance(outcome, dict)
        )
        if valid:
            observation = (
                outcome.get("verifier_observation")
                if isinstance(outcome, dict)
                else None
            )
            artifact_ids = receipt.get("artifact_ids")
            artifacts = outcome.get("artifacts") if isinstance(outcome, dict) else None
            satisfied = terminal.get("decision", {}).get("satisfied", [])
            workspace_state_before = prepared.get("workspace_state_before")
            workspace_state_after = commit.get("workspace_state_after")
            workspace_revision = commit.get("workspace_state_after", {}).get(
                "revision"
            )
            workspace_transition_valid = valid_workspace_transition(
                workspace_state_before, workspace_state_after, stable=True
            )
            workspace_revision_valid = (
                isinstance(workspace_revision, dict)
                and set(workspace_revision) == {"status", "sha256"}
                and workspace_revision.get("status") == "known"
                and valid_sha256(workspace_revision.get("sha256"))
            )
            artifacts_valid = (
                isinstance(artifact_ids, list)
                and bool(artifact_ids)
                and all(isinstance(artifact_id, str) for artifact_id in artifact_ids)
                and len(artifact_ids) == len(set(artifact_ids))
                and isinstance(artifacts, list)
                and all(isinstance(artifact, dict) for artifact in artifacts)
                and [artifact.get("id") for artifact in artifacts] == artifact_ids
                and all(
                    artifact.get("status") == "available"
                    and valid_sha256(artifact.get("sha256"))
                    and artifact.get("id")
                    == f"{VERIFICATION_ARTIFACT_PREFIX}{artifact.get('sha256')}"
                    and artifact.get("media_type")
                    == VERIFICATION_ARTIFACT_MEDIA_TYPE
                    and isinstance(artifact.get("byte_len"), int)
                    and not isinstance(artifact.get("byte_len"), bool)
                    and artifact["byte_len"] > 0
                    for artifact in artifacts
                )
            )
            observation_valid = (
                isinstance(observation, dict)
                and observation.get("spec") == verifier_spec()
                and observation.get("verdict") == "passed"
                and observation.get("workspace_revision") == workspace_revision
                and observation.get("artifact_ids") == artifact_ids
            )
            valid = all(
                (
                    outcome_axes
                    == {
                        "invocation": "accepted",
                        "transport": "succeeded",
                        "operation": "succeeded",
                        "side_effect": "indeterminate",
                        "retry": "not_needed",
                    },
                    outcome.get("evidence", {}).get("status") == "produced",
                    receipt.get("generation_id") == contract.get("generation_id"),
                    receipt.get("acceptance_id") == ACCEPTANCE_ID,
                    receipt.get("verification_id") == commit.get("verification_id"),
                    receipt.get("id") == f"receipt:{verification_id}",
                    receipt.get("verifier") == verifier_spec(),
                    receipt.get("workspace_state")
                    == commit.get("workspace_state_after")
                    == terminal.get("decision", {}).get("workspace_state"),
                    workspace_transition_valid,
                    observation_valid,
                    workspace_revision_valid,
                    outcome.get("workspace_revision")
                    == (
                        workspace_revision.get("sha256")
                        if isinstance(workspace_revision, dict)
                        else None
                    ),
                    outcome.get("evidence", {}).get("references") == artifact_ids,
                    artifacts_valid,
                    terminal.get("decision", {}).get("candidate_id")
                    == proposal.get("id"),
                    terminal.get("decision", {}).get("generation_id")
                    == generation_id,
                    satisfied
                    == [
                        {
                            "kind": "evidence",
                            "acceptance_id": ACCEPTANCE_ID,
                            "receipt_id": receipt.get("id"),
                        }
                    ],
                    not rejected,
                )
            )
            audit["receipt"] = {
                "id": receipt.get("id"),
                "generation_id": receipt.get("generation_id"),
                "acceptance_id": receipt.get("acceptance_id"),
                "verification_id": receipt.get("verification_id"),
                "verifier_spec_sha256": canonical_hash(receipt.get("verifier")),
                "workspace_state": receipt.get("workspace_state"),
                "artifact_ids": receipt.get("artifact_ids"),
                "artifacts": [
                    {
                        "id": artifact.get("id"),
                        "status": artifact.get("status"),
                        "sha256": artifact.get("sha256"),
                        "media_type": artifact.get("media_type"),
                        "byte_len": artifact.get("byte_len"),
                    }
                    for artifact in artifacts or []
                ],
            }
        return {
            "valid": valid,
            "exercised": True,
            "receipt_id": receipt.get("id") if isinstance(receipt, dict) else None,
            "rejection": False,
            "audit": audit,
        }

    evidence = outcome.get("evidence") if isinstance(outcome, dict) else None
    workspace_state_before = prepared.get("workspace_state_before")
    workspace_state_after = commit.get("workspace_state_after")
    workspace_transition_valid = valid_workspace_transition(
        workspace_state_before, workspace_state_after, stable=True
    )
    rejection_valid = (
        ordered
        and lifecycle_identity_valid
        and workspace_transition_valid
        and state == "blocked"
        and receipt is None
        and len(rejected) == 1
        and rejected[0].get("candidate_id") == proposal.get("id")
        and rejected[0].get("unmet_acceptance_ids") == [ACCEPTANCE_ID]
        and "completion_rejected" in kinds
        and isinstance(outcome, dict)
        and outcome_axes
        == {
            "invocation": "accepted",
            "transport": "succeeded",
            "operation": "failed",
            "side_effect": "indeterminate",
            "retry": "unsafe",
        }
        and isinstance(evidence, dict)
        and evidence.get("status") == "rejected"
        and evidence.get("references", []) == []
        and outcome.get("artifacts", []) == []
        and outcome.get("workspace_revision") is None
        and outcome.get("verifier_observation") is None
        and terminal.get("decision") is None
    )
    audit["rejection"] = rejected[0] if len(rejected) == 1 else None
    audit["available_artifact_count"] = len(outcome.get("artifacts", []))
    return {
        "valid": rejection_valid,
        "exercised": True,
        "receipt_id": None,
        "rejection": rejection_valid,
        "audit": audit,
    }


def accounting_metrics(run: dict[str, Any]) -> dict[str, Any]:
    accounting = run.get("accounting")
    usage = run.get("usage")
    if not isinstance(accounting, dict) or not isinstance(usage, dict):
        raise EvaluationError("run_accounting_missing")
    root = accounting.get("root", {})
    child = accounting.get("child", {})
    started = int(root.get("started", 0)) + int(child.get("started", 0))
    valid = all(
        (
            accounting.get("hard_request_limit") == MAX_API_REQUESTS,
            accounting.get("complete") is True,
            accounting.get("usage_complete") is True,
            accounting.get("usage_missing") is False,
            accounting.get("usage_incomplete") is False,
            accounting.get("billing_unknown") is False,
            accounting.get("unpriced") is False,
            accounting.get("budget_exhausted") is False,
            int(accounting.get("exhausted_denied", 0)) == 0,
            int(accounting.get("records_after_seal", 0)) == 0,
            int(root.get("in_flight", 0)) == 0,
            int(child.get("in_flight", 0)) == 0,
            accounting.get("usage") == usage,
            started <= MAX_API_REQUESTS,
            int(run.get("runtime_model_requests", 0)) <= MAX_API_REQUESTS,
        )
    )
    return {
        "valid": valid,
        "api_requests": started,
        "runtime_model_requests": run.get("runtime_model_requests"),
        "runtime_retries": run.get("runtime_retries"),
        "tool_calls": run.get("tool_calls"),
        "usage": usage,
        "cost_usd": round(int(accounting.get("cost_nanousd", 0)) / 1_000_000_000, 9),
        "cost_cny": round(int(accounting.get("cost_nanocny", 0)) / 1_000_000_000, 9),
        "hard_request_limit": accounting.get("hard_request_limit"),
    }


def run_one(
    variant: str,
    binary: Path,
    scenario: str,
    repetition: int,
    model: str,
    key: str,
) -> dict[str, Any]:
    schema_version = 4 if variant == "baseline" else 5
    started = time.monotonic()
    with tempfile.TemporaryDirectory(prefix=f"codewhale-m5-{variant}-") as raw:
        root = Path(raw)
        workspace = root / "workspace"
        state_root = root / "state"
        initial = initialize_workspace(workspace)
        initial_hash = canonical_hash(initial)
        server = StdioRunApi(
            binary,
            schema_version,
            workspace,
            child_environment(key, state_root),
        )
        try:
            result = server.request(
                start_command(variant, scenario, workspace, model), timeout=60
            )
            if result.get("kind") != "run":
                raise EvaluationError("start_did_not_return_run")
            run = result.get("run")
            if not isinstance(run, dict) or not isinstance(run.get("run_id"), str):
                raise EvaluationError("start_run_projection_missing")
            run_id = run["run_id"]
            deadline = time.monotonic() + MAX_RUNTIME_SECONDS + 30
            while run.get("terminal") is None:
                if time.monotonic() >= deadline:
                    raise EvaluationError("run_terminal_timeout")
                time.sleep(0.25)
                result = server.request({"kind": "get", "run_id": run_id})
                if result.get("kind") != "run":
                    raise EvaluationError("get_did_not_return_run")
                run = result["run"]
            result = server.request(
                {"kind": "events", "run_id": run_id, "after_sequence": 0}
            )
            if result.get("kind") != "events":
                raise EvaluationError("events_did_not_return_events")
            events = result.get("events")
            if not isinstance(events, list) or not events:
                raise EvaluationError("canonical_events_missing")
        finally:
            server.close()

        external = run_external_verifier(workspace)
        final = snapshot_workspace(workspace)
        state, _ = terminal_state(events)
        gate = (
            validate_candidate_gate(events, run, external["passed"])
            if variant == "candidate"
            else {
                "valid": True,
                "exercised": False,
                "receipt_id": None,
                "rejection": False,
                "audit": None,
            }
        )
        accounting = accounting_metrics(run)
        changed = changed_files(initial, final)
        event_schema_versions = sorted(
            {
                event.get("schema_version")
                for event in events
                if isinstance(event.get("schema_version"), int)
            }
        )
        expected_event_schema = 6 if variant == "baseline" else 7
        task_contract_valid = (
            run.get("task_contract") is None
            if variant == "baseline"
            else isinstance(run.get("task_contract"), dict)
            and run["task_contract"].get("definition") == task_definition(scenario)
            and isinstance(run["task_contract"].get("generation_id"), str)
            and bool(run["task_contract"]["generation_id"])
        )
        contract_valid = all(
            (
                run.get("model") == model,
                event_schema_versions == [expected_event_schema],
                task_contract_valid,
                accounting["valid"],
            )
        )
        verified_success = (
            state == "completed"
            and external["passed"]
            and (variant == "baseline" or gate["valid"])
        )
        false_success = state == "completed" and not external["passed"]
        return {
            "variant": variant,
            "scenario": scenario,
            "repetition": repetition,
            "schema_version": schema_version,
            "model": run.get("model"),
            "initial_workspace_sha256": initial_hash,
            "final_workspace_sha256": canonical_hash(final),
            "changed_files": changed,
            "terminal_state": state,
            "verified_success": verified_success,
            "false_success": false_success,
            "correct_rejection": gate["rejection"],
            "completion_gate_valid": gate["valid"],
            "completion_gate_exercised": gate["exercised"],
            "receipt_id": gate["receipt_id"],
            "completion_gate_audit": gate["audit"],
            "external_verifier": external,
            "accounting": accounting,
            "contract_valid": contract_valid,
            "measurement_valid": (
                accounting["valid"] and contract_valid and external["valid"]
            ),
            "duration_seconds": round(time.monotonic() - started, 3),
            "event_count": len(events),
            "event_schema_versions": event_schema_versions,
            "event_kind_counts": {
                kind: sum(event_kind(event) == kind for event in events)
                for kind in sorted({event_kind(event) for event in events})
            },
            "lifecycle_event_kinds": [
                event_kind(event)
                for event in events
                if event_kind(event) not in {"content_delta", "reasoning_delta"}
            ],
            "event_prefix_sha256": canonical_hash(events),
            "first_model_request_sha256": first_request_hash(events),
        }


def metric_summary(values: list[float | int]) -> dict[str, float | int]:
    return {
        "total": round(sum(values), 9),
        "mean": round(statistics.fmean(values), 9),
        "median": round(statistics.median(values), 9),
    }


def exact_cell_identity(records: list[dict[str, Any]], runs_per_cell: int) -> bool:
    expected = {
        (variant, scenario, repetition)
        for variant in ("baseline", "candidate")
        for scenario in SCENARIOS
        for repetition in range(1, runs_per_cell + 1)
    }
    observed = [
        (row.get("variant"), row.get("scenario"), row.get("repetition"))
        for row in records
    ]
    return len(observed) == len(expected) and set(observed) == expected


def aggregate(records: list[dict[str, Any]], runs_per_cell: int) -> dict[str, Any]:
    cells: dict[tuple[str, str], list[dict[str, Any]]] = defaultdict(list)
    for record in records:
        cells[(record["variant"], record["scenario"])].append(record)
    summaries: list[dict[str, Any]] = []
    for (variant, scenario), rows in sorted(cells.items()):
        api_requests = [int(row["accounting"]["api_requests"]) for row in rows]
        tokens = [
            int(row["accounting"]["usage"]["input_tokens"])
            + int(row["accounting"]["usage"]["output_tokens"])
            for row in rows
        ]
        costs = [float(row["accounting"]["cost_usd"]) for row in rows]
        durations = [float(row["duration_seconds"]) for row in rows]
        summaries.append(
            {
                "variant": variant,
                "scenario": scenario,
                "runs": len(rows),
                "verified_success": sum(bool(row["verified_success"]) for row in rows),
                "false_success": sum(bool(row["false_success"]) for row in rows),
                "correct_rejection": sum(bool(row["correct_rejection"]) for row in rows),
                "api_requests": metric_summary(api_requests),
                "tokens": metric_summary(tokens),
                "cost_usd": metric_summary(costs),
                "duration_seconds": metric_summary(durations),
                "measurement_valid": all(row["measurement_valid"] for row in rows),
                "contract_valid": all(row["contract_valid"] for row in rows),
                "completion_gate_valid": all(
                    row["completion_gate_valid"] for row in rows
                ),
            }
        )

    pair_prompt_equal = True
    by_pair: dict[tuple[str, int], dict[str, str | None]] = defaultdict(dict)
    for row in records:
        by_pair[(row["scenario"], row["repetition"])][row["variant"]] = row[
            "first_model_request_sha256"
        ]
    for pair in by_pair.values():
        pair_prompt_equal &= (
            set(pair) == {"baseline", "candidate"}
            and pair["baseline"] is not None
            and pair["baseline"] == pair["candidate"]
        )
    expected_cell_keys = {
        (variant, scenario)
        for variant in ("baseline", "candidate")
        for scenario in SCENARIOS
    }
    exact_repetitions = exact_cell_identity(records, runs_per_cell) and all(
        {int(row["repetition"]) for row in rows}
        == set(range(1, runs_per_cell + 1))
        and len(rows) == runs_per_cell
        for rows in cells.values()
    )
    initial_workspace_hashes = {
        row.get("initial_workspace_sha256") for row in records
    }
    frozen_fixture_hash = canonical_hash(snapshot_workspace(FIXTURE))
    initial_workspace_valid = initial_workspace_hashes == {frozen_fixture_hash}
    summaries_by_key = {
        (summary["variant"], summary["scenario"]): summary for summary in summaries
    }
    comparisons = []
    for scenario in SCENARIOS:
        baseline = summaries_by_key.get(("baseline", scenario))
        candidate = summaries_by_key.get(("candidate", scenario))
        if baseline is None or candidate is None:
            continue
        metrics = {}
        for metric in ("api_requests", "tokens", "cost_usd", "duration_seconds"):
            baseline_mean = float(baseline[metric]["mean"])
            candidate_mean = float(candidate[metric]["mean"])
            metrics[metric] = {
                "mean_delta": round(candidate_mean - baseline_mean, 9),
                "mean_delta_percent": (
                    round((candidate_mean - baseline_mean) / baseline_mean * 100, 4)
                    if baseline_mean
                    else None
                ),
            }
        comparisons.append({"scenario": scenario, "metrics": metrics})
    eligible = (
        set(cells) == expected_cell_keys
        and exact_repetitions
        and all(summary["measurement_valid"] for summary in summaries)
        and all(summary["contract_valid"] for summary in summaries)
        and initial_workspace_valid
        and all(
            row["completion_gate_valid"]
            and (
                row["variant"] == "baseline"
                or row["completion_gate_exercised"]
            )
            for row in records
        )
        and pair_prompt_equal
    )
    return {
        "product_metric_eligible": eligible,
        "pair_first_model_request_equal": pair_prompt_equal,
        "exact_cell_repetitions": exact_repetitions,
        "initial_workspace_valid": initial_workspace_valid,
        "cells": summaries,
        "comparisons": comparisons,
    }


def schedule(runs_per_cell: int) -> list[tuple[str, str, int]]:
    result: list[tuple[str, str, int]] = []
    for scenario_index, scenario in enumerate(SCENARIOS):
        for repetition in range(1, runs_per_cell + 1):
            order = (
                ("baseline", "candidate")
                if (scenario_index + repetition) % 2
                else ("candidate", "baseline")
            )
            result.extend((variant, scenario, repetition) for variant in order)
    return result


def atomic_write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    handle, raw_temp = tempfile.mkstemp(prefix=path.name + ".", dir=path.parent)
    temp = Path(raw_temp)
    try:
        with os.fdopen(handle, "wb") as stream:
            stream.write(canonical_bytes(payload))
            stream.write(b"\n")
            stream.flush()
            os.fsync(stream.fileno())
        os.chmod(temp, 0o600)
        os.replace(temp, path)
    finally:
        temp.unlink(missing_ok=True)


def default_output() -> Path:
    stamp = time.strftime("%Y%m%dT%H%M%SZ", time.gmtime())
    return ROOT / "eval/results" / f"m5-completion-gate-{stamp}.json"


def dry_plan(args: argparse.Namespace) -> dict[str, Any]:
    return {
        "schema": RESULT_SCHEMA,
        "mode": "dry_run",
        "model": args.model,
        "runs_per_cell": args.runs_per_cell,
        "planned_runs": len(SCENARIOS) * 2 * args.runs_per_cell,
        "schedule": [
            {"variant": variant, "scenario": scenario, "repetition": repetition}
            for variant, scenario, repetition in schedule(args.runs_per_cell)
        ],
        "fixture_sha256": canonical_hash(snapshot_workspace(FIXTURE)),
        "verifier_sha256": file_hash(VERIFIER),
        "verifier_spec_sha256": canonical_hash(verifier_spec()),
        "task_model_messages": {
            scenario: canonical_hash(model_message(task_definition(scenario)))
            for scenario in SCENARIOS
        },
    }


class HarnessSelfTests(unittest.TestCase):
    def test_source_revision_and_sha256_parsing_fail_closed(self) -> None:
        self.assertEqual(
            parse_version_revision("codewhale 0.8.68 (ba25f8368590)"),
            "ba25f8368590",
        )
        with self.assertRaises(EvaluationError):
            parse_version_revision("codewhale 0.8.68")
        self.assertTrue(valid_sha256("sha256:" + "a" * 64))
        self.assertFalse(valid_sha256("sha256:" + "A" * 64))
        self.assertFalse(valid_sha256("sha256:" + "a" * 63))

    def test_fixture_fails_and_known_fix_passes(self) -> None:
        with tempfile.TemporaryDirectory(prefix="m5-gate-self-test-") as raw:
            workspace = Path(raw) / "workspace"
            initialize_workspace(workspace)
            clean_status = subprocess.check_output(
                ["git", "status", "--porcelain=v1", "--untracked-files=all"],
                cwd=workspace,
                text=True,
            )
            self.assertFalse(run_external_verifier(workspace)["passed"])
            self.assertEqual(
                subprocess.check_output(
                    ["git", "status", "--porcelain=v1", "--untracked-files=all"],
                    cwd=workspace,
                    text=True,
                ),
                clean_status,
            )
            path = workspace / "ranges.py"
            body = path.read_text(encoding="utf-8")
            path.write_text(
                body.replace(
                    "        start = previous\n        previous = current",
                    "        start = current\n        previous = current",
                ),
                encoding="utf-8",
            )
            fixed_status = subprocess.check_output(
                ["git", "status", "--porcelain=v1", "--untracked-files=all"],
                cwd=workspace,
                text=True,
            )
            self.assertTrue(run_external_verifier(workspace)["passed"])
            self.assertEqual(
                subprocess.check_output(
                    ["git", "status", "--porcelain=v1", "--untracked-files=all"],
                    cwd=workspace,
                    text=True,
                ),
                fixed_status,
            )

    def test_model_message_and_exact_verifier_are_stable(self) -> None:
        task = task_definition("coding_fix")
        message = model_message(task)
        self.assertIn("任务目标：", message)
        self.assertIn("约束：", message)
        self.assertIn("非目标：", message)
        self.assertIn("Host 将使用 `run_verifiers`", message)
        spec = verifier_spec()
        self.assertEqual(spec["parameters"]["profile"], "exact")
        self.assertEqual(spec["parameters"]["level"], "quick")
        self.assertEqual(spec["plan"]["steps"][0]["timeout_ms"], 600_000)

    def test_schedule_has_balanced_complete_cells(self) -> None:
        planned = schedule(3)
        self.assertEqual(len(planned), 12)
        counts: dict[tuple[str, str], int] = defaultdict(int)
        for variant, scenario, _ in planned:
            counts[(variant, scenario)] += 1
        self.assertEqual(set(counts.values()), {3})

        identities = [
            {"variant": variant, "scenario": scenario, "repetition": repetition}
            for variant, scenario, repetition in planned
        ]
        self.assertTrue(exact_cell_identity(identities, 3))
        duplicate = identities[:-1] + [identities[0]]
        self.assertFalse(exact_cell_identity(duplicate, 3))
        self.assertFalse(exact_cell_identity(identities[:-1], 3))

    def test_external_verifier_health_is_not_a_task_failure(self) -> None:
        failed_checks = {name: True for name in EXPECTED_VERIFIER_CHECKS}
        failed_checks["hidden_cases"] = False
        payload = {
            "schema": "codewhale.eval.deepseek-exec-verifier.v1",
            "passed": False,
            "timed_out": False,
            "checks": failed_checks,
        }
        expected_failure = validate_external_verifier_payload(payload, 1)
        self.assertTrue(expected_failure["valid"])
        self.assertFalse(expected_failure["passed"])

        corrupt = dict(payload)
        corrupt["schema"] = "wrong"
        self.assertFalse(validate_external_verifier_payload(corrupt, 1)["valid"])
        missing = dict(payload)
        missing["checks"] = {"hidden_cases": False}
        self.assertFalse(validate_external_verifier_payload(missing, 1)["valid"])
        inconsistent = dict(payload)
        inconsistent["passed"] = True
        self.assertFalse(validate_external_verifier_payload(inconsistent, 0)["valid"])

    def test_candidate_gate_rejects_malformed_artifact_binding(self) -> None:
        generation_id = "generation:test"
        candidate_id = "completion:test"
        verification_id = (
            f"host-verification:{candidate_id}:{ACCEPTANCE_ID}"
        )
        artifact_sha256 = "sha256:" + "a" * 64
        artifact_id = f"{VERIFICATION_ARTIFACT_PREFIX}{artifact_sha256}"
        revision = {"status": "known", "sha256": "sha256:" + "b" * 64}
        before = {"generation": 0, "revision": revision}
        after = {"generation": 1, "revision": revision}
        candidate = {
            "id": candidate_id,
            "generation_id": generation_id,
            "message": "已完成",
        }
        receipt = {
            "id": f"receipt:{verification_id}",
            "generation_id": generation_id,
            "acceptance_id": ACCEPTANCE_ID,
            "verification_id": verification_id,
            "verifier": verifier_spec(),
            "workspace_state": after,
            "artifact_ids": [artifact_id],
        }
        outcome = {
            "invocation": "accepted",
            "transport": "succeeded",
            "operation": "succeeded",
            "side_effect": "indeterminate",
            "retry": "not_needed",
            "evidence": {"status": "produced", "references": [artifact_id]},
            "artifacts": [
                {
                    "id": artifact_id,
                    "status": "available",
                    "sha256": artifact_sha256,
                    "media_type": VERIFICATION_ARTIFACT_MEDIA_TYPE,
                    "byte_len": 42,
                }
            ],
            "workspace_revision": revision["sha256"],
            "verifier_observation": {
                "spec": verifier_spec(),
                "verdict": "passed",
                "workspace_revision": revision,
                "artifact_ids": [artifact_id],
            },
        }
        events = [
            {"event": {"kind": "completion_proposed", "candidate": candidate}},
            {
                "event": {
                    "kind": "host_verification_prepared",
                    "verification_id": verification_id,
                    "candidate": candidate,
                    "acceptance_id": ACCEPTANCE_ID,
                    "verifier": verifier_spec(),
                    "workspace_state_before": before,
                }
            },
            {
                "event": {
                    "kind": "host_verification_started",
                    "verification_id": verification_id,
                }
            },
            {
                "event": {
                    "kind": "host_verification_committed",
                    "verification_id": verification_id,
                    "outcome": outcome,
                    "receipt": receipt,
                    "workspace_state_after": after,
                }
            },
            {
                "event": {
                    "kind": "terminal",
                    "outcome": {
                        "terminal": {
                            "state": "completed",
                            "decision": {
                                "candidate_id": candidate_id,
                                "generation_id": generation_id,
                                "workspace_state": after,
                                "satisfied": [
                                    {
                                        "kind": "evidence",
                                        "acceptance_id": ACCEPTANCE_ID,
                                        "receipt_id": receipt["id"],
                                    }
                                ],
                            },
                        }
                    },
                }
            },
        ]
        run = {"task_contract": {"generation_id": generation_id}}
        self.assertTrue(validate_candidate_gate(events, run, True)["valid"])

        malformed = json.loads(json.dumps(events))
        malformed[3]["event"]["outcome"]["artifacts"][0]["sha256"] = (
            "sha256:" + "c" * 64
        )
        self.assertFalse(validate_candidate_gate(malformed, run, True)["valid"])

        failed = json.loads(json.dumps(events))
        failed_after = {"generation": 1, "revision": revision}
        committed = failed[3]["event"]
        committed["receipt"] = None
        committed["workspace_state_after"] = failed_after
        failed_outcome = committed["outcome"]
        failed_outcome["operation"] = "failed"
        failed_outcome["retry"] = "unsafe"
        failed_outcome["evidence"] = {"status": "rejected"}
        failed_outcome["artifacts"] = []
        failed_outcome.pop("workspace_revision")
        failed_outcome.pop("verifier_observation")
        failed.insert(
            4,
            {
                "event": {
                    "kind": "completion_rejected",
                    "rejection": {
                        "candidate_id": candidate_id,
                        "unmet_acceptance_ids": [ACCEPTANCE_ID],
                        "reason": "确定性 verifier 未通过",
                    },
                }
            },
        )
        failed[-1]["event"]["outcome"]["terminal"] = {"state": "blocked"}
        self.assertTrue(validate_candidate_gate(failed, run, False)["valid"])
        failed[4]["event"]["rejection"]["unmet_acceptance_ids"].append("extra")
        self.assertFalse(validate_candidate_gate(failed, run, False)["valid"])


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline-bin", type=Path)
    parser.add_argument("--candidate-bin", type=Path)
    parser.add_argument("--key-file", type=Path)
    parser.add_argument("--output", type=Path, default=default_output())
    parser.add_argument("--model", default=MODEL)
    parser.add_argument("--runs-per-cell", type=int, default=RUNS_PER_CELL)
    parser.add_argument("--acknowledge-cost", action="store_true")
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.self_test:
        suite = unittest.defaultTestLoader.loadTestsFromTestCase(HarnessSelfTests)
        return 0 if unittest.TextTestRunner(verbosity=2).run(suite).wasSuccessful() else 1
    if args.runs_per_cell < 3:
        raise EvaluationError("runs_per_cell_must_be_at_least_3")
    if args.dry_run:
        print(canonical_bytes(dry_plan(args)).decode("utf-8"))
        return 0
    if not args.acknowledge_cost:
        raise EvaluationError("live_run_requires_--acknowledge-cost")
    if args.baseline_bin is None or args.candidate_bin is None or args.key_file is None:
        raise EvaluationError("live_run_requires_baseline_candidate_and_key_file")

    baseline = executable_identity(args.baseline_bin)
    candidate = executable_identity(args.candidate_bin)
    if baseline["sha256"] == candidate["sha256"]:
        raise EvaluationError("baseline_and_candidate_must_differ")
    if baseline["source_revision"] == candidate["source_revision"]:
        raise EvaluationError("baseline_and_candidate_revisions_must_differ")
    key = load_key(args.key_file)
    records: list[dict[str, Any]] = []
    started_at = time.time()
    for index, (variant, scenario, repetition) in enumerate(
        schedule(args.runs_per_cell), start=1
    ):
        print(
            f"[{index}/{len(SCENARIOS) * 2 * args.runs_per_cell}] "
            f"{variant} {scenario} #{repetition}",
            flush=True,
        )
        binary = args.baseline_bin if variant == "baseline" else args.candidate_bin
        records.append(
            run_one(
                variant,
                binary.resolve(),
                scenario,
                repetition,
                args.model,
                key,
            )
        )

    result = {
        "schema": RESULT_SCHEMA,
        "evaluation_id": uuid.uuid4().hex,
        "created_at_unix": int(time.time()),
        "duration_seconds": round(time.time() - started_at, 3),
        "model": args.model,
        "runs_per_cell": args.runs_per_cell,
        "baseline": baseline,
        "candidate": candidate,
        "assets": {
            "fixture_sha256": canonical_hash(snapshot_workspace(FIXTURE)),
            "verifier_sha256": file_hash(VERIFIER),
            "verifier_spec_sha256": canonical_hash(verifier_spec()),
        },
        "records": records,
        "aggregate": aggregate(records, args.runs_per_cell),
    }
    atomic_write_json(args.output.resolve(), result)
    print(
        canonical_bytes(
            {
                "output": str(args.output.resolve()),
                "evaluation_id": result["evaluation_id"],
                "aggregate": result["aggregate"],
            }
        ).decode("utf-8")
    )
    return 0 if result["aggregate"]["product_metric_eligible"] else 2


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except EvaluationError as error:
        print(f"evaluation_error:{error}", file=sys.stderr)
        raise SystemExit(2) from None
