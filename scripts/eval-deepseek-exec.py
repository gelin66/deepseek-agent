#!/usr/bin/env python3
"""Run a cost-bounded baseline/candidate DeepSeek ``exec`` coding evaluation.

The harness evaluates explicit baseline and candidate binaries under identical
per-run budgets, with single-Agent and read-only-Explorer lanes kept as separate
comparison strata. It never checks out source or persists the NDJSON stream:
model content, reasoning, tool inputs/results, and the API key are consumed in
memory and discarded. Only redacted per-run measurements, deterministic
verifier evidence, cell aggregates, and lane-specific A/B deltas are emitted.
"""

from __future__ import annotations

import argparse
import dataclasses
import hashlib
import json
import os
import shutil
import signal
import stat
import statistics
import subprocess
import sys
import tempfile
import threading
import time
import unittest
import uuid
from pathlib import Path
from typing import Any, BinaryIO, TextIO


ROOT = Path(__file__).resolve().parent.parent
HARNESS_SOURCE_SHA256 = "sha256:" + hashlib.sha256(
    Path(__file__).resolve().read_bytes()
).hexdigest()
FIXTURE_ROOT = ROOT / "eval" / "fixtures" / "deepseek-exec"
FIXTURE_WORKSPACE = FIXTURE_ROOT / "workspace"
VERIFIER = FIXTURE_ROOT / "verifier.py"
AGGREGATE_FIXTURE = FIXTURE_ROOT / "aggregate-runs.json"

SCHEMA = "codewhale.eval.deepseek-exec.v2"
TASK_ID = "python-coalesce-ranges-v1"
DEFAULT_MODEL = "deepseek-v4-flash"
SUPPORTED_MODELS = ("deepseek-v4-flash", "deepseek-v4-pro")
PRICE_SNAPSHOT = "2026-07-16"
OFFICIAL_PRICES_USD = {
    "deepseek-v4-flash": {"hit": 0.0028, "miss": 0.14, "output": 0.28},
    "deepseek-v4-pro": {"hit": 0.003625, "miss": 0.435, "output": 0.87},
}
COST_MATCH_ABS_TOLERANCE_USD = 1e-9
COST_MATCH_REL_TOLERANCE = 1e-6

# Both lanes receive these exact three limits. The request budget is shared by
# root, child, retries, and recovery inside the production runtime.
MAX_API_REQUESTS_PER_LANE = 10
MAX_RUNTIME_SECONDS_PER_LANE = 360
MAX_TURNS_PER_LANE = 32
OUTER_PROCESS_GRACE_SECONDS = 45
MIN_RUNS_PER_CELL = 3
DEFAULT_RUNS_PER_CELL = 3
MAX_RUNS_PER_CELL = 20
PER_RUN_COST_CEILING_USD = 0.025
MAX_STREAM_BYTES = 64 * 1024 * 1024
MAX_STREAM_LINE_BYTES = 4 * 1024 * 1024
VERIFIER_TIMEOUT_SECONDS = 30
ACTIVE_OUTPUT_STREAM: TextIO | None = None
SCHEDULE_POLICY = "deterministic_pair_order_balance_v1"

ALLOWED_TOOLS = (
    "read_file",
    "list_dir",
    "grep_files",
    "apply_patch",
    "edit_file",
    "exec_shell",
    "agent",
)
# Shell is conservatively write-capable. Treat every invocation as a write
# barrier so a root Agent cannot mutate the workspace before the Explorer
# handoff and still pass merely because the edit was hidden inside a command.
WRITE_TOOLS = {"apply_patch", "write_file", "edit_file", "fim_edit", "exec_shell"}
MUTATION_TOOLS = {"apply_patch", "write_file", "edit_file", "fim_edit"}
EXPECTED_CHANGED_FILES = ["ranges.py"]
ENVIRONMENT_ALLOWLIST = (
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
HOST_ENVIRONMENT_NAMES = {
    "PATH",
    "TMPDIR",
    "TMP",
    "TEMP",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
}
TERMINAL_REASON_STATUS = {
    "resolved": "completed",
    "canceled": "interrupted",
    "unresolved": "failed",
    "stuck": "failed",
    "timeout": "failed",
    "budget_exhausted": "failed",
    "approval_required": "failed",
    "model_error": "failed",
    "tool_error": "failed",
    "infrastructure_error": "failed",
    "evidence_missing": "failed",
}

BASE_TASK = """任务：修复当前 Python 项目中的范围合并缺陷。

验收要求：
1. 先理解 `coalesce_ranges` 的契约和失败测试；
2. 只修改 `ranges.py`，不得修改测试或 README，不得新增文件；
3. 运行 `python3 -m unittest -q`，确认测试真实通过；
4. 不要读取环境变量、凭据或仓库外文件；
5. 完成后给出简短结果，不要伪造测试结论。
"""

SINGLE_PROMPT = BASE_TASK + """
执行方式：本 lane 必须由根 Agent 独立完成，不得启动任何子 Agent。
"""

MULTI_PROMPT = BASE_TASK + """
执行方式：这是固定的多 Agent lane，必须遵守以下顺序：
1. 立即且只启动一个子 Agent；调用 `agent` 时必须显式传入 `type=explore`、
   `fork_context=false`、`expected_artifact="缺陷诊断"`、`max_steps=4`、
   `wall_time_secs=120`，并将
   `allowed_tools` 严格设为 `["read_file", "list_dir", "grep_files"]`，让它只读检查缺陷；
   不得启动第二个子 Agent；
2. 启动后本轮不要再调用任何工具，让 Runtime 自动等待 Explorer 并回注 handoff；
   handoff 到达前根 Agent 不得读取、修改文件或运行命令；
3. 收到 handoff 后，只有根 Agent 可以通过 `apply_patch` 或 `edit_file` 修改
   `ranges.py`；不得用测试 shell 命令冒充修改；然后由根 Agent 运行测试并完成任务。
"""


def sha256_bytes(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return "sha256:" + digest.hexdigest()


def json_hash(value: Any) -> str:
    return sha256_bytes(
        json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()
    )


def emit(record: dict[str, Any], stream: TextIO | None = None) -> None:
    destination = stream or ACTIVE_OUTPUT_STREAM or sys.stdout
    print(
        json.dumps({"schema": SCHEMA, **record}, ensure_ascii=True, sort_keys=True),
        file=destination,
        flush=True,
    )


def fail_preflight(code: str, *, key_accessed: bool = False) -> int:
    emit(
        {
            "record_type": "error",
            "error_code": code,
            "key_accessed": key_accessed,
            "network_accessed": False,
        },
        ACTIVE_OUTPUT_STREAM or sys.stderr,
    )
    return 2


def git_metadata() -> dict[str, Any]:
    try:
        commit = subprocess.run(
            ["git", "rev-parse", "HEAD"],
            cwd=ROOT,
            text=True,
            capture_output=True,
            check=True,
        ).stdout.strip()
        dirty = bool(
            subprocess.run(
                ["git", "status", "--porcelain"],
                cwd=ROOT,
                text=True,
                capture_output=True,
                check=True,
            ).stdout
        )
    except (OSError, subprocess.CalledProcessError):
        return {"harness_git_commit": None, "harness_git_dirty": None}
    return {"harness_git_commit": commit, "harness_git_dirty": dirty}


def ignored_workspace_path(relative: Path) -> bool:
    parts = relative.parts
    runtime_state = len(parts) >= 2 and parts[:2] == (".codewhale", "state")
    root_git = bool(parts) and parts[0] == ".git"
    return root_git or "__pycache__" in parts or runtime_state


def snapshot_workspace(workspace: Path) -> dict[str, dict[str, Any]]:
    snapshot: dict[str, dict[str, Any]] = {}
    for path in sorted(workspace.rglob("*")):
        relative_path = path.relative_to(workspace)
        if ignored_workspace_path(relative_path):
            continue
        relative = relative_path.as_posix()
        if path.is_symlink():
            snapshot[relative] = {
                "kind": "symlink",
                "git_mode": "120000",
                "sha256": sha256_bytes(
                    ("symlink\0" + os.readlink(path)).encode("utf-8", "surrogateescape")
                ),
            }
        elif path.is_file():
            mode = stat.S_IMODE(path.stat(follow_symlinks=False).st_mode)
            snapshot[relative] = {
                "kind": "file",
                "git_mode": "100755" if mode & 0o111 else "100644",
                "sha256": sha256_file(path),
            }
    return snapshot


def workspace_hash(snapshot: dict[str, dict[str, Any]]) -> str:
    return json_hash(snapshot)


def changed_files(
    before: dict[str, dict[str, Any]], after: dict[str, dict[str, Any]]
) -> list[str]:
    return sorted(
        path for path in before.keys() | after.keys() if before.get(path) != after.get(path)
    )


def diff_hash(
    before: dict[str, dict[str, Any]], after: dict[str, dict[str, Any]]
) -> str:
    changes = [
        {"path": path, "before": before.get(path), "after": after.get(path)}
        for path in changed_files(before, after)
    ]
    return json_hash(changes)


def load_key(path: Path) -> str:
    raw = path.read_bytes()
    if len(raw) > 4096:
        raise ValueError("key_too_large")
    try:
        key = raw.decode("utf-8").strip()
    except UnicodeDecodeError as error:
        raise ValueError("key_not_utf8") from error
    if not key:
        raise ValueError("missing_api_key")
    if any(character.isspace() or ord(character) < 32 for character in key):
        raise ValueError("invalid_key_format")
    return key


def resolve_binary(value: str) -> Path:
    # `codewhale` is the public production dispatcher. It delegates `exec` to
    # the sibling `codewhale-tui`; terminal metadata records that actual loop
    # binary hash in addition to this launcher hash.
    candidate = Path(value)
    if not candidate.is_absolute():
        candidate = ROOT / candidate
    return candidate.resolve()


def sibling_runtime_binary(dispatcher: Path) -> Path:
    suffix = ".exe" if dispatcher.suffix.lower() == ".exe" else ""
    preferred = dispatcher.with_name(f"codewhale-tui{suffix}")
    if preferred.is_file() or not suffix:
        return preferred
    fallback = dispatcher.with_name("codewhale-tui")
    return fallback if fallback.is_file() else preferred


def binary_pair_identity(dispatcher: Path) -> dict[str, Any]:
    runtime = sibling_runtime_binary(dispatcher)
    launcher_ready = dispatcher.is_file() and os.access(dispatcher, os.X_OK)
    runtime_ready = runtime.is_file() and os.access(runtime, os.X_OK)
    launcher_sha256 = sha256_file(dispatcher) if launcher_ready else None
    runtime_sha256 = sha256_file(runtime) if runtime_ready else None
    pair_sha256 = (
        json_hash(
            {
                "launcher_sha256": launcher_sha256,
                "runtime_sha256": runtime_sha256,
            }
        )
        if launcher_sha256 and runtime_sha256
        else None
    )
    return {
        "launcher": str(dispatcher),
        "launcher_exists": dispatcher.is_file(),
        "launcher_executable": launcher_ready,
        "launcher_sha256": launcher_sha256,
        "runtime": str(runtime),
        "runtime_exists": runtime.is_file(),
        "runtime_executable": runtime_ready,
        "runtime_sha256": runtime_sha256,
        "pair_sha256": pair_sha256,
    }


@dataclasses.dataclass(frozen=True)
class EvaluationTarget:
    variant: str
    binary: Path
    revision: str

    def plan_manifest(self) -> dict[str, Any]:
        identity = binary_pair_identity(self.binary)
        return {
            "variant": self.variant,
            "binary": str(self.binary),
            "binary_exists": identity["launcher_exists"],
            "binary_executable": identity["launcher_executable"],
            "binary_sha256": identity["launcher_sha256"],
            "runtime_binary": identity["runtime"],
            "runtime_binary_exists": identity["runtime_exists"],
            "runtime_binary_executable": identity["runtime_executable"],
            "runtime_binary_sha256": identity["runtime_sha256"],
            "binary_pair_sha256": identity["pair_sha256"],
            "revision": self.revision,
            "revision_attestation": "operator_supplied",
        }


@dataclasses.dataclass(frozen=True)
class FrozenEvaluationTarget:
    source: EvaluationTarget
    execution: EvaluationTarget
    launcher_sha256: str
    runtime_sha256: str
    pair_sha256: str


@dataclasses.dataclass(frozen=True)
class FrozenEvaluationAssets:
    fixture_workspace: Path
    verifier: Path
    fixture_sha256: str
    verifier_sha256: str


def freeze_target(target: EvaluationTarget, destination_root: Path) -> FrozenEvaluationTarget:
    source_identity = binary_pair_identity(target.binary)
    if not source_identity["launcher_executable"] or not source_identity["runtime_executable"]:
        raise ValueError(f"{target.variant}_binary_pair_unavailable")
    destination = destination_root / target.variant
    destination.mkdir(parents=True, exist_ok=False)
    launcher = destination / target.binary.name
    runtime_source = Path(str(source_identity["runtime"]))
    runtime = destination / runtime_source.name
    shutil.copy2(target.binary, launcher)
    shutil.copy2(runtime_source, runtime)
    launcher.chmod(launcher.stat().st_mode | stat.S_IXUSR)
    runtime.chmod(runtime.stat().st_mode | stat.S_IXUSR)
    execution = EvaluationTarget(target.variant, launcher.resolve(), target.revision)
    frozen_identity = binary_pair_identity(execution.binary)
    if (
        frozen_identity["launcher_sha256"] != source_identity["launcher_sha256"]
        or frozen_identity["runtime_sha256"] != source_identity["runtime_sha256"]
        or frozen_identity["pair_sha256"] != source_identity["pair_sha256"]
    ):
        raise ValueError(f"{target.variant}_binary_pair_copy_mismatch")
    return FrozenEvaluationTarget(
        source=target,
        execution=execution,
        launcher_sha256=str(frozen_identity["launcher_sha256"]),
        runtime_sha256=str(frozen_identity["runtime_sha256"]),
        pair_sha256=str(frozen_identity["pair_sha256"]),
    )


def freeze_evaluation_assets(destination_root: Path) -> FrozenEvaluationAssets:
    destination = destination_root / "assets"
    fixture = destination / "workspace"
    verifier = destination / "verifier.py"
    destination.mkdir(parents=True, exist_ok=False)
    source_snapshot = snapshot_workspace(FIXTURE_WORKSPACE)
    shutil.copytree(FIXTURE_WORKSPACE, fixture, symlinks=True)
    shutil.copy2(VERIFIER, verifier)
    copied_snapshot = snapshot_workspace(fixture)
    if source_snapshot != copied_snapshot or sha256_file(VERIFIER) != sha256_file(verifier):
        raise ValueError("evaluation_asset_copy_mismatch")
    return FrozenEvaluationAssets(
        fixture_workspace=fixture,
        verifier=verifier,
        fixture_sha256=workspace_hash(copied_snapshot),
        verifier_sha256=sha256_file(verifier),
    )


def configured_targets(args: argparse.Namespace) -> tuple[list[EvaluationTarget], list[str]]:
    targets: list[EvaluationTarget] = []
    errors: list[str] = []
    pairs = (
        ("baseline", args.baseline_binary, args.baseline_revision),
        ("candidate", args.candidate_binary, args.candidate_revision),
    )
    for variant, binary_value, revision_value in pairs:
        binary_present = bool(binary_value)
        revision = revision_value.strip() if isinstance(revision_value, str) else ""
        revision_present = bool(revision)
        if binary_present != revision_present:
            errors.append(f"{variant}_target_incomplete")
            continue
        if binary_present and revision_present:
            targets.append(
                EvaluationTarget(
                    variant=variant,
                    binary=resolve_binary(binary_value),
                    revision=revision,
                )
            )
    return targets, errors


def target_by_variant(
    targets: list[EvaluationTarget], variant: str
) -> EvaluationTarget | None:
    return next((target for target in targets if target.variant == variant), None)


def execution_budget(lane: str, model: str) -> dict[str, Any]:
    return {
        "task_id": TASK_ID,
        "lane": lane,
        "model": model,
        "provider": "deepseek",
        "reasoning_effort": "high",
        "sandbox": "workspace-write",
        "allowed_tools": list(ALLOWED_TOOLS),
        "max_api_requests": MAX_API_REQUESTS_PER_LANE,
        "max_runtime_seconds": MAX_RUNTIME_SECONDS_PER_LANE,
        "max_turns": MAX_TURNS_PER_LANE,
        "outer_process_grace_seconds": OUTER_PROCESS_GRACE_SECONDS,
        "cost_ceiling_usd": PER_RUN_COST_CEILING_USD,
        "prompt_sha256": sha256_bytes(lane_prompt(lane).encode("utf-8")),
    }


def planned_eligibility(
    targets: list[EvaluationTarget], runs_per_cell: int
) -> tuple[bool, list[str]]:
    reasons: list[str] = []
    if target_by_variant(targets, "baseline") is None:
        reasons.append("baseline_required")
    if target_by_variant(targets, "candidate") is None:
        reasons.append("candidate_required")
    if runs_per_cell < MIN_RUNS_PER_CELL:
        reasons.append("minimum_runs_per_cell_not_met")
    baseline = target_by_variant(targets, "baseline")
    candidate = target_by_variant(targets, "candidate")
    if (
        baseline is not None
        and candidate is not None
        and baseline.revision == candidate.revision
    ):
        reasons.append("baseline_candidate_revision_must_differ")
    if baseline is not None and candidate is not None:
        baseline_pair = binary_pair_identity(baseline.binary).get("pair_sha256")
        candidate_pair = binary_pair_identity(candidate.binary).get("pair_sha256")
        if baseline_pair is not None and baseline_pair == candidate_pair:
            reasons.append("baseline_candidate_binary_pair_must_differ")
    for target in targets:
        identity = binary_pair_identity(target.binary)
        if not identity["launcher_executable"]:
            reasons.append(f"{target.variant}_binary_unavailable")
        if not identity["runtime_executable"]:
            reasons.append(f"{target.variant}_runtime_binary_unavailable")
    return not reasons, reasons


def lane_prompt(lane: str) -> str:
    return SINGLE_PROMPT if lane == "single" else MULTI_PROMPT


def exec_command(binary: Path, lane: str, model: str) -> list[str]:
    # The API key is intentionally absent. It is injected only into the child
    # environment immediately before Popen.
    return [
        str(binary),
        "--provider",
        "deepseek",
        "--model",
        model,
        "exec",
        "--reasoning-effort",
        "high",
        "--auto",
        "--sandbox",
        "workspace-write",
        "--output-format",
        "stream-json",
        "--allowed-tools",
        ",".join(ALLOWED_TOOLS),
        "--max-turns",
        str(MAX_TURNS_PER_LANE),
        "--max-api-requests",
        str(MAX_API_REQUESTS_PER_LANE),
        "--max-runtime-secs",
        str(MAX_RUNTIME_SECONDS_PER_LANE),
        lane_prompt(lane),
    ]


def sanitized_host_environment() -> dict[str, str]:
    environment = {
        name: os.environ[name]
        for name in ENVIRONMENT_ALLOWLIST
        if name in HOST_ENVIRONMENT_NAMES and name in os.environ
    }
    environment.update(
        {
            "PYTHONDONTWRITEBYTECODE": "1",
            "GIT_CONFIG_NOSYSTEM": "1",
        }
    )
    return environment


def child_environment(key: str, state_root: Path) -> dict[str, str]:
    # Start from an explicit allowlist. In particular, never inherit
    # DEEPSEEK_TUI_BIN/CODEWHALE_* overrides that could silently replace the
    # frozen dispatcher/runtime pair under evaluation.
    environment = {
        name: os.environ[name]
        for name in ENVIRONMENT_ALLOWLIST
        if name in os.environ
    }
    home = state_root / "home"
    app_home = state_root / "codewhale"
    xdg = state_root / "xdg"
    for directory in (home, app_home, xdg):
        directory.mkdir(parents=True, exist_ok=True)
    environment.update(
        {
            "HOME": str(home),
            "USERPROFILE": str(home),
            "CODEWHALE_HOME": str(app_home),
            "XDG_CONFIG_HOME": str(xdg),
            "DEEPSEEK_API_KEY": key,
            "NO_COLOR": "1",
            "PYTHONDONTWRITEBYTECODE": "1",
            "RUST_BACKTRACE": "0",
        }
    )
    return environment


def initialize_fixture_workspace(
    destination: Path, fixture_workspace: Path = FIXTURE_WORKSPACE
) -> dict[str, dict[str, Any]]:
    shutil.copytree(fixture_workspace, destination, symlinks=True)
    initial = snapshot_workspace(destination)
    environment = sanitized_host_environment()
    environment.update(
        {
            "GIT_AUTHOR_DATE": "2026-07-15T00:00:00Z",
            "GIT_COMMITTER_DATE": "2026-07-15T00:00:00Z",
        }
    )
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
            "fixture baseline",
        ],
    )
    for command in commands:
        subprocess.run(
            command,
            cwd=destination,
            env=environment,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=True,
        )
    return initial


def optional_int(value: Any) -> int | None:
    return value if isinstance(value, int) and not isinstance(value, bool) else None


def optional_number(value: Any) -> float | None:
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        return float(value)
    return None


def optional_bool(value: Any) -> bool | None:
    return value if isinstance(value, bool) else None


def parse_spawn_agent_id(event: dict[str, Any]) -> str | None:
    if event.get("status") != "success" or not isinstance(event.get("output"), str):
        return None
    try:
        payload = json.loads(event["output"])
    except json.JSONDecodeError:
        return None
    agent_id = payload.get("agent_id") if isinstance(payload, dict) else None
    return agent_id if isinstance(agent_id, str) and agent_id else None


def is_verification_invocation(value: Any) -> bool:
    if not isinstance(value, dict):
        return False
    command = next(
        (value.get(name) for name in ("cmd", "command") if isinstance(value.get(name), str)),
        None,
    )
    if command is None:
        return False
    normalized = " ".join(command.strip().split())
    return normalized == "python3 -m unittest -q"


def canonical_agent_spawn(value: Any) -> bool:
    return (
        isinstance(value, dict)
        and "action" not in value
        and "op" not in value
    )


def valid_explorer_spawn(value: Any) -> bool:
    if not isinstance(value, dict):
        return False
    canonical_fields = {
        "prompt",
        "type",
        "fork_context",
        "allowed_tools",
        "max_steps",
        "max_depth",
        "wall_time_secs",
        "expected_artifact",
    }
    return (
        canonical_agent_spawn(value)
        and set(value).issubset(canonical_fields)
        and isinstance(value.get("prompt"), str)
        and bool(value["prompt"].strip())
        and str(value.get("type", "")).strip().lower() == "explore"
        and value.get("fork_context") is False
        and value.get("expected_artifact") == "缺陷诊断"
        and value.get("max_steps") == 4
        and value.get("wall_time_secs") == 120
        and isinstance(value.get("allowed_tools"), list)
        and all(isinstance(name, str) for name in value["allowed_tools"])
        and set(value.get("allowed_tools", []))
        == {"read_file", "list_dir", "grep_files"}
    )


@dataclasses.dataclass
class StreamReceipt:
    event_count: int = 0
    stream_bytes: int = 0
    invalid_lines: int = 0
    schema_errors: int = 0
    oversized_lines: int = 0
    overflow: bool = False
    last_type: str | None = None
    done_count: int = 0
    terminal_count: int = 0
    terminal: dict[str, Any] | None = None
    events_after_terminal: int = 0
    error_count: int = 0
    errors_before_terminal: int = 0
    error_termination_reasons: list[str | None] = dataclasses.field(
        default_factory=list
    )
    error_code: str | None = None
    tool_calls: int = 0
    tool_failures: int = 0
    agent_spawn_count: int = 0
    valid_explorer_spawn_count: int = 0
    agent_spawn_success_count: int = 0
    agent_tool_call_count: int = 0
    child_started_count: int = 0
    child_finished_count: int = 0
    first_child_finished_index: int | None = None
    write_indices: list[int] = dataclasses.field(default_factory=list)
    successful_mutation_indices: list[int] = dataclasses.field(default_factory=list)
    patch_calls: int = 0
    patch_failures: int = 0
    verification_runs: int = 0
    verification_failures: int = 0
    first_tool_name: str | None = None
    first_tool_valid_explorer: bool = False
    pre_handoff_root_tool_count: int = 0
    pre_handoff_root_tool_names: list[str] = dataclasses.field(default_factory=list)
    duplicate_tool_ids: int = 0
    orphan_tool_results: int = 0
    tool_name_mismatches: int = 0
    started_tools: dict[str, tuple[str, int, bool, int]] = dataclasses.field(
        default_factory=dict
    )
    tool_lifecycle_receipt: list[dict[str, Any]] = dataclasses.field(default_factory=list)
    agent_spawn_tool_ids: list[str] = dataclasses.field(default_factory=list)
    spawned_agent_ids: list[str] = dataclasses.field(default_factory=list)
    started_children: list[tuple[str, str]] = dataclasses.field(default_factory=list)
    started_child_indices: list[int] = dataclasses.field(default_factory=list)
    started_child_depths: list[int] = dataclasses.field(default_factory=list)
    finished_children: list[tuple[str, str]] = dataclasses.field(default_factory=list)
    finished_child_indices: list[int] = dataclasses.field(default_factory=list)
    child_finished_statuses: list[str] = dataclasses.field(default_factory=list)
    completed_child_result_proven: bool = False
    handoff_workspace_snapshot: dict[str, dict[str, Any]] | None = None
    handoff_snapshot_error: bool = False

    def process(self, event: Any, workspace: Path | None = None) -> None:
        if not isinstance(event, dict):
            self.invalid_lines += 1
            return
        self.event_count += 1
        event_type = event.get("type")
        if not isinstance(event_type, str):
            self.schema_errors += 1
            return
        self.last_type = event_type
        if event.get("schema") != "codewhale.exec-stream" or event.get("schema_version") != 1:
            self.schema_errors += 1

        # A failed terminal has one narrow envelope: typed terminal metadata,
        # one matching error, then done. Every other event remains forbidden
        # after terminal metadata.
        if self.terminal_count > 0 and event_type not in {"error", "done"}:
            self.events_after_terminal += 1

        if event_type == "done":
            self.done_count += 1
            return
        if event_type == "metadata":
            meta = event.get("meta")
            if isinstance(meta, dict) and meta.get("receipt_kind") == "terminal":
                self.terminal_count += 1
                if self.terminal is None:
                    self.terminal = sanitize_terminal(meta)
            return
        if event_type == "error":
            self.error_count += 1
            if self.terminal_count == 0:
                self.errors_before_terminal += 1
            reason = event.get("termination_reason")
            self.error_termination_reasons.append(
                reason if isinstance(reason, str) else None
            )
            code = event.get("code")
            if isinstance(code, str):
                self.error_code = code
            return
        if event_type == "child_started":
            call_id = event.get("call_id")
            child_run_id = event.get("child_run_id")
            depth = optional_int(event.get("depth"))
            if (
                not isinstance(call_id, str)
                or not call_id
                or not isinstance(child_run_id, str)
                or not child_run_id
                or depth is None
                or depth < 1
            ):
                self.schema_errors += 1
                return
            self.child_started_count += 1
            self.started_children.append((call_id, child_run_id))
            self.started_child_indices.append(self.event_count)
            self.started_child_depths.append(depth)
            return
        if event_type == "child_finished":
            call_id = event.get("call_id")
            child_run_id = event.get("child_run_id")
            status = event.get("status")
            result_present = event.get("result_present")
            if (
                not isinstance(call_id, str)
                or not call_id
                or not isinstance(child_run_id, str)
                or not child_run_id
                or not isinstance(status, str)
                or status not in {
                    "completed",
                    "blocked",
                    "failed",
                    "cancelled",
                    "interrupted",
                    "recovery_required",
                }
                or not isinstance(result_present, bool)
            ):
                self.schema_errors += 1
                return
            self.child_finished_count += 1
            self.finished_children.append((call_id, child_run_id))
            self.finished_child_indices.append(self.event_count)
            self.child_finished_statuses.append(status)
            if status == "completed" and result_present:
                self.completed_child_result_proven = True
            if self.first_child_finished_index is None:
                self.first_child_finished_index = self.event_count
                if workspace is None:
                    self.handoff_snapshot_error = True
                else:
                    try:
                        self.handoff_workspace_snapshot = snapshot_workspace(workspace)
                    except OSError:
                        self.handoff_snapshot_error = True
            return
        if event_type == "tool_use":
            self.tool_calls += 1
            name = event.get("name")
            tool_id = event.get("id")
            if not isinstance(name, str) or not isinstance(tool_id, str):
                self.schema_errors += 1
                return
            tool_input = event.get("input")
            if self.first_tool_name is None:
                self.first_tool_name = name
                self.first_tool_valid_explorer = name == "agent" and valid_explorer_spawn(
                    tool_input
                )
            if tool_id in self.started_tools:
                self.duplicate_tool_ids += 1
                return
            verification = name == "exec_shell" and is_verification_invocation(tool_input)
            lifecycle_index = len(self.tool_lifecycle_receipt)
            self.tool_lifecycle_receipt.append(
                {
                    "ordinal": lifecycle_index + 1,
                    "use_event_ordinal": self.event_count,
                    "name": name,
                    "status": None,
                }
            )
            self.started_tools[tool_id] = (
                name,
                self.event_count,
                verification,
                lifecycle_index,
            )
            if name in WRITE_TOOLS:
                self.write_indices.append(self.event_count)
            if name in MUTATION_TOOLS:
                self.patch_calls += 1
            valid_explorer = name == "agent" and valid_explorer_spawn(tool_input)
            if name == "agent":
                self.agent_tool_call_count += 1
            exempt_first_spawn = valid_explorer and self.agent_tool_call_count == 1
            if self.first_child_finished_index is None and not exempt_first_spawn:
                self.pre_handoff_root_tool_count += 1
                if name not in self.pre_handoff_root_tool_names:
                    self.pre_handoff_root_tool_names.append(name)
            if name == "agent":
                if canonical_agent_spawn(tool_input):
                    self.agent_spawn_count += 1
                    self.agent_spawn_tool_ids.append(tool_id)
                    self.valid_explorer_spawn_count += int(valid_explorer_spawn(tool_input))
            return
        if event_type == "tool_result":
            tool_id = event.get("id")
            if not isinstance(tool_id, str):
                self.schema_errors += 1
                return
            started = self.started_tools.pop(tool_id, None)
            if started is None:
                self.orphan_tool_results += 1
                return
            started_name, started_index, verification, lifecycle_index = started
            event_name = event.get("name")
            if not isinstance(event_name, str):
                self.schema_errors += 1
            elif event_name != started_name:
                self.tool_name_mismatches += 1
            name = started_name
            status = event.get("status")
            if not isinstance(status, str):
                self.schema_errors += 1
                receipt_status = "schema_error"
            else:
                receipt_status = status
            self.tool_lifecycle_receipt[lifecycle_index]["status"] = receipt_status
            self.tool_lifecycle_receipt[lifecycle_index][
                "result_event_ordinal"
            ] = self.event_count
            if status != "success":
                self.tool_failures += 1
                if name in MUTATION_TOOLS:
                    self.patch_failures += 1
            elif (
                name in MUTATION_TOOLS
                and event.get("side_effect_status") == "applied"
            ):
                self.successful_mutation_indices.append(started_index)
            if verification:
                self.verification_runs += 1
                if event.get("status") != "success":
                    self.verification_failures += 1
            if (
                name == "agent"
                and event.get("status") == "success"
                and tool_id in self.agent_spawn_tool_ids
            ):
                agent_id = parse_spawn_agent_id(event)
                if agent_id is not None:
                    self.agent_spawn_success_count += 1
                    self.spawned_agent_ids.append(agent_id)

    def protocol_errors(self) -> list[str]:
        errors: list[str] = []
        if self.invalid_lines:
            errors.append("invalid_json_or_shape")
        if self.schema_errors:
            errors.append("stream_schema_mismatch")
        if self.oversized_lines:
            errors.append("stream_line_too_large")
        if self.overflow:
            errors.append("stream_too_large")
        if self.duplicate_tool_ids:
            errors.append("duplicate_tool_id")
        if self.orphan_tool_results:
            errors.append("orphan_tool_result")
        if self.tool_name_mismatches:
            errors.append("tool_name_mismatch")
        if self.started_tools:
            errors.append("tool_result_missing")
        if len(set(self.started_children)) != len(self.started_children):
            errors.append("duplicate_child_started")
        if len(set(self.finished_children)) != len(self.finished_children):
            errors.append("duplicate_child_finished")
        if any(pair not in self.started_children for pair in self.finished_children):
            errors.append("orphan_child_finished")
        started_at = dict(zip(self.started_children, self.started_child_indices))
        if any(
            started_at.get(pair, self.event_count + 1) >= finished_index
            for pair, finished_index in zip(
                self.finished_children, self.finished_child_indices
            )
        ):
            errors.append("child_lifecycle_out_of_order")
        if self.terminal_count != 1:
            errors.append("terminal_metadata_count")
        if self.done_count != 1:
            errors.append("done_count")
        if self.last_type != "done":
            errors.append("done_not_last")
        if self.events_after_terminal:
            errors.append("event_after_terminal_metadata")
        if self.errors_before_terminal:
            errors.append("error_before_terminal_metadata")
        if self.terminal_count == 1 and self.terminal is not None:
            status = self.terminal.get("status")
            if status == "completed":
                if self.error_count:
                    errors.append("completed_terminal_with_error")
            else:
                if not terminal_status_reason_valid(self.terminal):
                    errors.append("failure_terminal_status_reason_mismatch")
                if self.error_count != 1:
                    errors.append("failure_error_count")
                elif self.error_termination_reasons != [
                    self.terminal.get("termination_reason")
                ]:
                    errors.append("error_termination_reason_mismatch")
        return errors


def sanitize_terminal(meta: dict[str, Any]) -> dict[str, Any]:
    text_fields = (
        "provider",
        "model",
        "route_source",
        "approval_posture",
        "sandbox_posture",
        "binary_sha256",
        "prompt_sha256",
        "tool_catalog_sha256",
        "status",
        "termination_reason",
        "error_category",
    )
    integer_fields = (
        "duration_ms",
        "input_tokens",
        "output_tokens",
        "prompt_cache_hit_tokens",
        "prompt_cache_miss_tokens",
        "prompt_cache_write_tokens",
        "reasoning_tokens",
        "reasoning_replay_tokens",
        "total_tokens",
        "usage_response_count",
        "standard_chat_response_count",
        "strict_chat_response_count",
        "fim_response_count",
        "usage_missing_responses",
        "usage_incomplete_responses",
        "billing_unknown_attempts",
        "unpriced_usage_responses",
        "transport_retry_count",
        "api_request_count",
        "api_request_completed",
        "api_request_in_flight",
        "api_request_root_started",
        "api_request_root_completed",
        "api_request_root_in_flight",
        "api_request_child_started",
        "api_request_child_completed",
        "api_request_child_in_flight",
        "api_request_limit",
        "api_request_rejected_exhausted",
        "usage_records_after_seal_observed",
        "api_request_rejected_after_seal_observed",
    )
    boolean_fields = (
        "usage_complete",
        "cost_complete",
        "api_request_budget_exhausted",
    )
    result: dict[str, Any] = {}
    for field in text_fields:
        if isinstance(meta.get(field), str):
            result[field] = meta[field]
    for field in integer_fields:
        result[field] = optional_int(meta.get(field))
    for field in boolean_fields:
        result[field] = optional_bool(meta.get(field))
    result["cost_usd"] = optional_number(meta.get("cost_usd"))
    result["cost_cny"] = optional_number(meta.get("cost_cny"))
    buckets: list[dict[str, Any]] = []
    raw_buckets = meta.get("surface_model_usage_buckets")
    if isinstance(raw_buckets, list):
        bucket_integer_fields = (
            "response_count",
            "usage_response_count",
            "input_tokens",
            "output_tokens",
            "prompt_cache_hit_tokens",
            "prompt_cache_miss_tokens",
            "prompt_cache_write_tokens",
            "reasoning_tokens",
            "reasoning_replay_tokens",
            "total_tokens",
        )
        for raw in raw_buckets:
            if not isinstance(raw, dict):
                continue
            bucket: dict[str, Any] = {}
            for field in ("model", "api_surface"):
                if isinstance(raw.get(field), str):
                    bucket[field] = raw[field]
            for field in bucket_integer_fields:
                bucket[field] = optional_int(raw.get(field))
            bucket["cost_usd"] = optional_number(raw.get("cost_usd"))
            bucket["cost_cny"] = optional_number(raw.get("cost_cny"))
            buckets.append(bucket)
    result["surface_model_usage_buckets"] = buckets
    return result


def read_stdout(stream: BinaryIO, receipt: StreamReceipt, workspace: Path) -> None:
    for raw in iter(stream.readline, b""):
        receipt.stream_bytes += len(raw)
        if receipt.stream_bytes > MAX_STREAM_BYTES:
            receipt.overflow = True
            continue
        if len(raw) > MAX_STREAM_LINE_BYTES:
            receipt.oversized_lines += 1
            continue
        try:
            event = json.loads(raw)
        except (UnicodeDecodeError, json.JSONDecodeError):
            receipt.invalid_lines += 1
            continue
        receipt.process(event, workspace)


def stop_process(process: subprocess.Popen[bytes]) -> None:
    if process.poll() is not None:
        return
    if os.name != "nt":
        try:
            os.killpg(process.pid, signal.SIGTERM)
        except ProcessLookupError:
            return
    else:
        process.terminate()
    try:
        process.wait(timeout=2)
        return
    except subprocess.TimeoutExpired:
        pass
    if os.name != "nt":
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            return
    else:
        process.kill()


@dataclasses.dataclass
class ProcessOutcome:
    returncode: int | None
    timed_out: bool
    spawn_error: bool
    wall_time_ms: int
    stream: StreamReceipt


def run_exec_process(
    command: list[str], environment: dict[str, str], workspace: Path
) -> ProcessOutcome:
    receipt = StreamReceipt()
    started = time.monotonic()
    try:
        process = subprocess.Popen(
            command,
            cwd=workspace,
            env=environment,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            # Human diagnostics can contain model/tool text. The structured
            # terminal event is the only accepted status source.
            stderr=subprocess.DEVNULL,
            start_new_session=os.name != "nt",
        )
    except OSError:
        return ProcessOutcome(None, False, True, 0, receipt)
    assert process.stdout is not None
    stdout_thread = threading.Thread(
        target=read_stdout,
        args=(process.stdout, receipt, workspace),
        daemon=True,
    )
    stdout_thread.start()

    deadline = started + MAX_RUNTIME_SECONDS_PER_LANE + OUTER_PROCESS_GRACE_SECONDS
    timed_out = False
    while process.poll() is None:
        if receipt.overflow or time.monotonic() >= deadline:
            timed_out = time.monotonic() >= deadline
            stop_process(process)
            break
        time.sleep(0.05)
    try:
        process.wait(timeout=3)
    except subprocess.TimeoutExpired:
        stop_process(process)
        process.wait(timeout=3)
    stdout_thread.join(timeout=3)
    return ProcessOutcome(
        process.returncode,
        timed_out,
        False,
        int((time.monotonic() - started) * 1000),
        receipt,
    )


def run_verifier(workspace: Path, verifier_path: Path = VERIFIER) -> dict[str, Any]:
    started = time.monotonic()
    try:
        completed = subprocess.run(
            [sys.executable, "-I", str(verifier_path), str(workspace)],
            cwd=ROOT,
            env=sanitized_host_environment(),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            timeout=VERIFIER_TIMEOUT_SECONDS,
            check=False,
        )
    except subprocess.TimeoutExpired:
        return {
            "id": "python-coalesce-ranges-v1",
            "sha256": sha256_file(verifier_path),
            "passed": False,
            "timed_out": True,
            "exit_code": None,
            "duration_ms": int((time.monotonic() - started) * 1000),
            "checks": {},
        }
    try:
        payload = json.loads(completed.stdout)
    except (UnicodeDecodeError, json.JSONDecodeError):
        payload = {}
    checks = payload.get("checks") if isinstance(payload, dict) else None
    safe_checks = (
        {str(name): value for name, value in checks.items() if isinstance(value, bool)}
        if isinstance(checks, dict)
        else {}
    )
    passed = (
        completed.returncode == 0
        and isinstance(payload, dict)
        and payload.get("schema") == "codewhale.eval.deepseek-exec-verifier.v1"
        and payload.get("passed") is True
        and bool(safe_checks)
        and all(safe_checks.values())
    )
    return {
        "id": "python-coalesce-ranges-v1",
        "sha256": sha256_file(verifier_path),
        "passed": passed,
        "timed_out": bool(payload.get("timed_out")) if isinstance(payload, dict) else False,
        "exit_code": completed.returncode,
        "duration_ms": int((time.monotonic() - started) * 1000),
        "checks": safe_checks,
    }


def lane_contract(
    lane: str,
    receipt: StreamReceipt,
    initial_snapshot: dict[str, dict[str, Any]] | None = None,
) -> dict[str, Any]:
    if lane == "single":
        passed = (
            receipt.agent_tool_call_count == 0
            and receipt.agent_spawn_count == 0
            and receipt.child_started_count == 0
            and receipt.child_finished_count == 0
        )
        return {
            "passed": passed,
            "evidence_complete": True,
            "expected_child_count": 0,
            "agent_tool_calls": receipt.agent_tool_call_count,
            "child_spawn_count": receipt.agent_spawn_count,
            "child_started_receipts": receipt.child_started_count,
            "child_finished_receipts": receipt.child_finished_count,
        }
    handoff_index = receipt.first_child_finished_index
    writes_after_handoff = (
        sum(index > handoff_index for index in receipt.write_indices)
        if handoff_index is not None
        else 0
    )
    writes_before_handoff = (
        sum(index < handoff_index for index in receipt.write_indices)
        if handoff_index is not None
        else len(receipt.write_indices)
    )
    successful_mutations_after_handoff = (
        sum(index > handoff_index for index in receipt.successful_mutation_indices)
        if handoff_index is not None
        else 0
    )
    spawned_id = (
        receipt.spawned_agent_ids[0] if len(receipt.spawned_agent_ids) == 1 else None
    )
    started_pair = (
        receipt.started_children[0] if len(receipt.started_children) == 1 else None
    )
    finished_pair = (
        receipt.finished_children[0] if len(receipt.finished_children) == 1 else None
    )
    child_receipts_match = (
        spawned_id is not None
        and started_pair is not None
        and finished_pair is not None
        and len(receipt.agent_spawn_tool_ids) == 1
        and started_pair[0] == receipt.agent_spawn_tool_ids[0]
        and started_pair == finished_pair
        and spawned_id == started_pair[1]
    )
    agent_lifecycles = [
        item for item in receipt.tool_lifecycle_receipt if item.get("name") == "agent"
    ]
    child_receipts_in_order = (
        len(agent_lifecycles) == 1
        and len(receipt.started_child_indices) == 1
        and len(receipt.finished_child_indices) == 1
        and isinstance(agent_lifecycles[0].get("use_event_ordinal"), int)
        and isinstance(agent_lifecycles[0].get("result_event_ordinal"), int)
        and agent_lifecycles[0]["use_event_ordinal"]
        < receipt.started_child_indices[0]
        < agent_lifecycles[0]["result_event_ordinal"]
        < receipt.finished_child_indices[0]
    )
    root_child_depth_valid = receipt.started_child_depths == [1]
    child_completed = receipt.child_finished_statuses == ["completed"]
    workspace_unchanged_at_handoff = bool(
        initial_snapshot is not None
        and receipt.handoff_workspace_snapshot is not None
        and receipt.handoff_workspace_snapshot == initial_snapshot
        and not receipt.handoff_snapshot_error
    )
    reasons: list[str] = []
    if not receipt.first_tool_valid_explorer:
        reasons.append("first_tool_must_be_canonical_read_only_explorer")
    if receipt.agent_tool_call_count != 1:
        reasons.append("exactly_one_agent_tool_call_required")
    if receipt.agent_spawn_count != 1 or receipt.valid_explorer_spawn_count != 1:
        reasons.append("exactly_one_canonical_read_only_explorer_required")
    if receipt.agent_spawn_success_count != 1 or len(receipt.spawned_agent_ids) != 1:
        reasons.append("spawn_receipt_missing_or_ambiguous")
    if receipt.child_started_count != 1 or len(receipt.started_children) != 1:
        reasons.append("one_child_started_receipt_required")
    if receipt.child_finished_count != 1 or len(receipt.finished_children) != 1:
        reasons.append("one_child_finished_receipt_required")
    if not child_completed:
        reasons.append("child_must_complete")
    if not child_receipts_match:
        reasons.append("spawn_child_receipt_mismatch")
    if not child_receipts_in_order:
        reasons.append("child_lifecycle_order_invalid")
    if not root_child_depth_valid:
        reasons.append("root_child_depth_must_be_one")
    if receipt.pre_handoff_root_tool_count:
        reasons.append("root_tool_before_handoff")
    if not workspace_unchanged_at_handoff:
        reasons.append("workspace_changed_or_unprovable_at_handoff")
    if writes_before_handoff:
        reasons.append("write_capable_tool_before_handoff")
    if successful_mutations_after_handoff < 1:
        reasons.append("root_mutation_after_handoff_required")
    if not receipt.completed_child_result_proven:
        reasons.append("child_result_receipt_missing")
    passed = not reasons and (
        receipt.agent_tool_call_count == 1
        and receipt.agent_spawn_count == 1
        and receipt.valid_explorer_spawn_count == 1
        and receipt.agent_spawn_success_count == 1
        and receipt.child_started_count == 1
        and receipt.child_finished_count == 1
        and writes_before_handoff == 0
        and successful_mutations_after_handoff >= 1
    )
    return {
        "passed": passed,
        "evidence_complete": initial_snapshot is not None
        and not receipt.handoff_snapshot_error,
        "reasons": reasons,
        "expected_child_count": 1,
        "agent_tool_calls": receipt.agent_tool_call_count,
        "child_spawn_count": receipt.agent_spawn_count,
        "valid_read_only_explorer_spawns": receipt.valid_explorer_spawn_count,
        "successful_child_spawns": receipt.agent_spawn_success_count,
        "child_started_receipts": receipt.child_started_count,
        "child_finished_receipts": receipt.child_finished_count,
        "first_tool": receipt.first_tool_name,
        "first_tool_valid_explorer": receipt.first_tool_valid_explorer,
        "spawned_agent_id_count": len(receipt.spawned_agent_ids),
        "started_child_id_count": len(receipt.started_children),
        "finished_child_id_count": len(receipt.finished_children),
        "spawn_child_receipts_match": child_receipts_match,
        "child_lifecycle_order_valid": child_receipts_in_order,
        "root_child_depth_valid": root_child_depth_valid,
        "child_completed": child_completed,
        "child_result_proven": receipt.completed_child_result_proven,
        "workspace_unchanged_at_handoff": workspace_unchanged_at_handoff,
        "root_tools_before_handoff": receipt.pre_handoff_root_tool_count,
        "root_tool_names_before_handoff": receipt.pre_handoff_root_tool_names,
        "writes_before_handoff": writes_before_handoff,
        "writes_after_handoff": writes_after_handoff,
        "successful_mutations_after_handoff": successful_mutations_after_handoff,
    }


def valid_sha256(value: Any) -> bool:
    if not isinstance(value, str) or not value.startswith("sha256:") or len(value) != 71:
        return False
    return all(character in "0123456789abcdef" for character in value[7:])


def official_cost_usd(
    model: str, cache_hit_tokens: int, cache_miss_tokens: int, output_tokens: int
) -> float | None:
    price = OFFICIAL_PRICES_USD.get(model)
    if price is None or min(cache_hit_tokens, cache_miss_tokens, output_tokens) < 0:
        return None
    return (
        cache_hit_tokens * price["hit"]
        + cache_miss_tokens * price["miss"]
        + output_tokens * price["output"]
    ) / 1_000_000


def costs_close(left: float | None, right: float | None) -> bool:
    if left is None or right is None:
        return False
    difference = abs(left - right)
    return difference <= max(
        COST_MATCH_ABS_TOLERANCE_USD,
        COST_MATCH_REL_TOLERANCE * max(abs(left), abs(right)),
    )


def classify_claim_outcome(
    terminal_claimed_success: bool, claim_acceptance_passed: bool
) -> tuple[bool, bool]:
    verified_success = bool(claim_acceptance_passed)
    false_success = bool(
        terminal_claimed_success and not claim_acceptance_passed
    )
    return verified_success, false_success


def terminal_status_reason_valid(terminal: dict[str, Any] | None) -> bool:
    if not terminal:
        return False
    reason = terminal.get("termination_reason")
    status = terminal.get("status")
    return isinstance(reason, str) and TERMINAL_REASON_STATUS.get(reason) == status


def terminal_execution_contract_valid(
    terminal: dict[str, Any] | None,
    requested_model: str,
    expected_runtime_sha256: str,
) -> bool:
    return bool(
        terminal
        and terminal.get("provider") == "deepseek"
        and terminal.get("model") == requested_model
        and terminal.get("route_source") == "explicit_or_configured"
        and terminal.get("approval_posture") == "auto_tools"
        and terminal.get("sandbox_posture") == "workspace-write"
        and terminal.get("binary_sha256") == expected_runtime_sha256
        and valid_sha256(terminal.get("tool_catalog_sha256"))
        and terminal.get("api_request_limit") == MAX_API_REQUESTS_PER_LANE
    )


def actor_request_accounting_valid(
    terminal: dict[str, Any], *, required: bool = False, lane: str | None = None
) -> bool:
    request_count = optional_int(terminal.get("api_request_count"))
    completed = optional_int(terminal.get("api_request_completed"))
    in_flight = optional_int(terminal.get("api_request_in_flight"))
    actor_fields = {
        field: optional_int(terminal.get(field))
        for field in (
            "api_request_root_started",
            "api_request_root_completed",
            "api_request_root_in_flight",
            "api_request_child_started",
            "api_request_child_completed",
            "api_request_child_in_flight",
        )
    }
    actor_values = list(actor_fields.values())
    if all(value is None for value in actor_values):
        return not required
    if any(value is None or value < 0 for value in actor_values):
        return False
    exact = bool(
        actor_fields["api_request_root_started"]
        + actor_fields["api_request_child_started"]
        == request_count
        and actor_fields["api_request_root_completed"]
        + actor_fields["api_request_child_completed"]
        == completed
        and actor_fields["api_request_root_in_flight"]
        + actor_fields["api_request_child_in_flight"]
        == in_flight
    )
    if not exact:
        return False
    if not required:
        return True
    root_started = actor_fields["api_request_root_started"]
    root_completed = actor_fields["api_request_root_completed"]
    root_in_flight = actor_fields["api_request_root_in_flight"]
    child_started = actor_fields["api_request_child_started"]
    child_completed = actor_fields["api_request_child_completed"]
    child_in_flight = actor_fields["api_request_child_in_flight"]
    if not (
        root_started > 0
        and root_completed == root_started
        and root_in_flight == 0
        and child_completed == child_started
        and child_in_flight == 0
    ):
        return False
    if lane == "multi":
        return child_started > 0
    if lane == "single":
        return child_started == 0
    return False


def terminal_measurement_valid(
    terminal: dict[str, Any] | None,
    *,
    require_actor_accounting: bool = False,
    lane: str | None = None,
) -> bool:
    if not terminal:
        return False
    actual_model = terminal.get("model")
    if actual_model not in SUPPORTED_MODELS or not terminal_status_reason_valid(terminal):
        return False
    request_count = optional_int(terminal.get("api_request_count"))
    completed = optional_int(terminal.get("api_request_completed"))
    if not actor_request_accounting_valid(
        terminal, required=require_actor_accounting, lane=lane
    ):
        return False
    request_limit = optional_int(terminal.get("api_request_limit"))
    rejected_exhausted = optional_int(terminal.get("api_request_rejected_exhausted"))
    transport_retries = optional_int(terminal.get("transport_retry_count"))
    budget_exhausted = optional_bool(terminal.get("api_request_budget_exhausted"))
    input_tokens = optional_int(terminal.get("input_tokens"))
    output_tokens = optional_int(terminal.get("output_tokens"))
    total_tokens = optional_int(terminal.get("total_tokens"))
    cache_hit = optional_int(terminal.get("prompt_cache_hit_tokens"))
    cache_miss = optional_int(terminal.get("prompt_cache_miss_tokens"))
    usage_responses = optional_int(terminal.get("usage_response_count"))
    surface_counts = [
        optional_int(terminal.get(field))
        for field in (
            "standard_chat_response_count",
            "strict_chat_response_count",
            "fim_response_count",
        )
    ]
    buckets = terminal.get("surface_model_usage_buckets")
    if not isinstance(buckets, list):
        return False
    bucket_integer_fields = (
        "response_count",
        "usage_response_count",
        "input_tokens",
        "output_tokens",
        "prompt_cache_hit_tokens",
        "prompt_cache_miss_tokens",
        "total_tokens",
    )
    if any(
        not isinstance(bucket, dict)
        or bucket.get("model") != actual_model
        or bucket.get("api_surface") not in {"standard_chat", "strict_chat", "fim"}
        or any(optional_int(bucket.get(field)) is None for field in bucket_integer_fields)
        or any(int(bucket[field]) < 0 for field in bucket_integer_fields)
        or optional_number(bucket.get("cost_usd")) is None
        or optional_number(bucket.get("cost_cny")) is None
        for bucket in buckets
    ):
        return False
    if len({(bucket["model"], bucket["api_surface"]) for bucket in buckets}) != len(
        buckets
    ):
        return False
    bucket_sums = {
        field: sum(int(bucket[field]) for bucket in buckets)
        for field in bucket_integer_fields
    }
    bucket_surface_responses = {
        surface: sum(
            int(bucket["response_count"])
            for bucket in buckets
            if bucket.get("api_surface") == surface
        )
        for surface in ("standard_chat", "strict_chat", "fim")
    }
    runtime_cost = optional_number(terminal.get("cost_usd"))
    runtime_cost_cny = optional_number(terminal.get("cost_cny"))
    repriced_cost = (
        official_cost_usd(actual_model, cache_hit, cache_miss, output_tokens)
        if None not in (cache_hit, cache_miss, output_tokens)
        else None
    )
    optional_bucket_projection_valid = all(
        (
            optional_int(terminal.get(field)) is None
            and all(optional_int(bucket.get(field)) is None for bucket in buckets)
        )
        or (
            optional_int(terminal.get(field)) is not None
            and all(optional_int(bucket.get(field)) is not None for bucket in buckets)
            and sum(int(bucket[field]) for bucket in buckets) == terminal[field]
        )
        for field in (
            "prompt_cache_write_tokens",
            "reasoning_tokens",
            "reasoning_replay_tokens",
        )
    )
    return (
        request_count is not None
        and request_limit is not None
        and 0 <= request_count <= request_limit
        and request_limit > 0
        and completed == request_count
        and terminal.get("api_request_in_flight") == 0
        and budget_exhausted is not None
        and rejected_exhausted is not None
        and rejected_exhausted >= 0
        and budget_exhausted == (rejected_exhausted > 0)
        and transport_retries is not None
        and transport_retries >= 0
        and terminal.get("api_request_rejected_after_seal_observed") == 0
        and terminal.get("usage_complete") is True
        and terminal.get("cost_complete") is True
        and terminal.get("usage_missing_responses") == 0
        and terminal.get("usage_incomplete_responses") == 0
        and terminal.get("billing_unknown_attempts") == 0
        and terminal.get("unpriced_usage_responses") == 0
        and terminal.get("usage_records_after_seal_observed") == 0
        and runtime_cost is not None
        and runtime_cost_cny is not None
        and None not in (input_tokens, output_tokens, total_tokens, cache_hit, cache_miss)
        and total_tokens == input_tokens + output_tokens
        and input_tokens == cache_hit + cache_miss
        and None not in surface_counts
        and usage_responses == sum(surface_counts)
        and usage_responses is not None
        and 0 <= usage_responses <= request_count
        and bucket_sums["response_count"] == usage_responses
        and bucket_sums["usage_response_count"] == usage_responses
        and bucket_sums["input_tokens"] == input_tokens
        and bucket_sums["output_tokens"] == output_tokens
        and bucket_sums["prompt_cache_hit_tokens"] == cache_hit
        and bucket_sums["prompt_cache_miss_tokens"] == cache_miss
        and bucket_sums["total_tokens"] == total_tokens
        and bucket_surface_responses["standard_chat"] == surface_counts[0]
        and bucket_surface_responses["strict_chat"] == surface_counts[1]
        and bucket_surface_responses["fim"] == surface_counts[2]
        and costs_close(
            sum(float(bucket["cost_usd"]) for bucket in buckets), runtime_cost
        )
        and costs_close(
            sum(float(bucket["cost_cny"]) for bucket in buckets), runtime_cost_cny
        )
        and costs_close(runtime_cost, repriced_cost)
        and optional_bucket_projection_valid
    )


def run_lane(
    lane: str,
    frozen: FrozenEvaluationTarget,
    assets: FrozenEvaluationAssets,
    repetition_index: int,
    pair_order: str,
    pair_position: int,
    schedule_position: int,
    model: str,
    key: str,
    common: dict[str, Any],
) -> dict[str, Any]:
    target = frozen.source
    with tempfile.TemporaryDirectory(
        prefix=f"codewhale-deepseek-exec-{target.variant}-{lane}-{repetition_index}-"
    ) as temporary:
        temporary_path = Path(temporary)
        workspace = temporary_path / "workspace"
        state_root = temporary_path / "state"
        initial_snapshot = initialize_fixture_workspace(
            workspace, assets.fixture_workspace
        )
        initial_hash = workspace_hash(initial_snapshot)
        fixture_matches_frozen = initial_hash == assets.fixture_sha256
        frozen_pair_identity = binary_pair_identity(frozen.execution.binary)
        frozen_pair_matches = (
            frozen_pair_identity.get("launcher_sha256") == frozen.launcher_sha256
            and frozen_pair_identity.get("runtime_sha256") == frozen.runtime_sha256
            and frozen_pair_identity.get("pair_sha256") == frozen.pair_sha256
        )
        prompt = lane_prompt(lane)
        prompt_sha256 = sha256_bytes(prompt.encode("utf-8"))
        command = exec_command(frozen.execution.binary, lane, model)
        environment = child_environment(key, state_root)
        process = run_exec_process(command, environment, workspace)
        # Drop the only environment container holding the credential before
        # verifier/diff work. Neither argv nor any record ever receives it.
        environment.clear()

        verifier_input_snapshot = snapshot_workspace(workspace)
        verifier_input_hash = workspace_hash(verifier_input_snapshot)
        verifier = run_verifier(workspace, assets.verifier)
        final_snapshot = snapshot_workspace(workspace)
        final_hash = workspace_hash(final_snapshot)
        verifier_workspace_stable = verifier_input_snapshot == final_snapshot
        verifier["workspace_revision_sha256"] = verifier_input_hash
        verifier["workspace_revision_stable"] = verifier_workspace_stable
        changed = changed_files(initial_snapshot, final_snapshot)
        receipt = process.stream
        protocol_errors = receipt.protocol_errors()
        terminal = receipt.terminal
        prompt_matches = bool(terminal and terminal.get("prompt_sha256") == prompt_sha256)
        actor_accounting_required = target.variant == "candidate"
        actor_accounting_valid = bool(
            terminal
            and actor_request_accounting_valid(
                terminal, required=actor_accounting_required, lane=lane
            )
        )
        measurement_valid = terminal_measurement_valid(
            terminal,
            require_actor_accounting=actor_accounting_required,
            lane=lane,
        )
        execution_contract_valid = terminal_execution_contract_valid(
            terminal, model, frozen.runtime_sha256
        )
        contract = lane_contract(lane, receipt, initial_snapshot)
        terminal_claimed_success = bool(terminal and terminal.get("status") == "completed")
        runtime_success = bool(
            terminal_claimed_success
            and terminal is not None
            and terminal.get("termination_reason") == "resolved"
            and process.returncode == 0
            and not protocol_errors
            and receipt.error_count == 0
        )
        task_evidence_passed = bool(
            verifier["passed"]
            and verifier.get("sha256") == assets.verifier_sha256
            and verifier_workspace_stable
            and fixture_matches_frozen
            and changed == EXPECTED_CHANGED_FILES
        )
        task_contract_passed = bool(
            task_evidence_passed and prompt_matches and contract["passed"]
        )
        accounting = terminal or {}
        runtime_reported_cost_usd = optional_number(accounting.get("cost_usd"))
        cache_hit = optional_int(accounting.get("prompt_cache_hit_tokens"))
        cache_miss = optional_int(accounting.get("prompt_cache_miss_tokens"))
        output_tokens = optional_int(accounting.get("output_tokens"))
        priced_model = accounting.get("model")
        run_cost_usd = (
            official_cost_usd(priced_model, cache_hit, cache_miss, output_tokens)
            if priced_model in SUPPORTED_MODELS
            and None not in (cache_hit, cache_miss, output_tokens)
            else None
        )
        runtime_cost_matches = costs_close(run_cost_usd, runtime_reported_cost_usd)
        cost_within_ceiling = (
            run_cost_usd is not None and run_cost_usd <= PER_RUN_COST_CEILING_USD
        )
        claim_acceptance_passed = bool(
            runtime_success
            and task_evidence_passed
            and prompt_matches
            and measurement_valid
            and execution_contract_valid
            and runtime_cost_matches
            and frozen_pair_matches
            and contract["passed"]
            and cost_within_ceiling
            and not process.timed_out
            and not process.spawn_error
        )
        verified_success, false_success = classify_claim_outcome(
            terminal_claimed_success, claim_acceptance_passed
        )
        lane_passed = verified_success
        budget = execution_budget(lane, model)
        run_id = (
            f"{common['evaluation_id']}:{target.variant}:{lane}:{repetition_index}"
        )
        return {
            **common,
            "record_type": "run_manifest",
            "product_metric_eligible": False,
            "eligibility_scope": "cell_and_lane_comparison_only",
            "run_id": run_id,
            "cell_id": f"{target.variant}:{lane}",
            "variant": target.variant,
            "target_revision": target.revision,
            "revision_attestation": "operator_supplied",
            "repetition_index": repetition_index,
            "pair_order": pair_order,
            "pair_position": pair_position,
            "schedule_position": schedule_position,
            "lane": lane,
            "status": "passed" if lane_passed else "failed",
            "verified_success": verified_success,
            "task_evidence_passed": task_evidence_passed,
            "task_contract_passed": task_contract_passed,
            "claim_acceptance_passed": claim_acceptance_passed,
            "false_success": false_success,
            "terminal_claimed_success": terminal_claimed_success,
            "process_exit_code": process.returncode,
            "process_timed_out": process.timed_out,
            "process_spawn_error": process.spawn_error,
            "wall_time_ms": process.wall_time_ms,
            "terminal_duration_ms": accounting.get("duration_ms"),
            "terminal_status": accounting.get("status"),
            "termination_reason": accounting.get("termination_reason"),
            "error_category": accounting.get("error_category"),
            "error_code": receipt.error_code,
            "stream_contract": {
                "passed": not protocol_errors,
                "errors": protocol_errors,
                "event_count": receipt.event_count,
                "terminal_metadata_count": receipt.terminal_count,
                "done_count": receipt.done_count,
                "done_last": receipt.last_type == "done",
                "stream_bytes": receipt.stream_bytes,
                "duplicate_tool_ids": receipt.duplicate_tool_ids,
                "orphan_tool_results": receipt.orphan_tool_results,
                "tool_name_mismatches": receipt.tool_name_mismatches,
                "open_tool_calls": len(receipt.started_tools),
                "stderr_policy": "discarded",
            },
            "lane_contract": contract,
            "prompt_sha256": prompt_sha256,
            "terminal_prompt_sha256_matches": prompt_matches,
            "route_source": accounting.get("route_source"),
            "approval_posture": accounting.get("approval_posture"),
            "sandbox_posture": accounting.get("sandbox_posture"),
            "tool_catalog_sha256": accounting.get("tool_catalog_sha256"),
            "runtime_binary_sha256": accounting.get("binary_sha256"),
            "target_binary": str(target.binary),
            "frozen_target_binary": str(frozen.execution.binary),
            "target_binary_sha256": frozen.launcher_sha256,
            "binary_pair_sha256": frozen.pair_sha256,
            "frozen_binary_pair_matches": frozen_pair_matches,
            "execution_budget": budget,
            "execution_budget_sha256": json_hash(budget),
            "workspace_initial_sha256": initial_hash,
            "fixture_matches_frozen": fixture_matches_frozen,
            "workspace_verifier_input_sha256": verifier_input_hash,
            "workspace_final_sha256": final_hash,
            "verifier_workspace_stable": verifier_workspace_stable,
            "diff_sha256": diff_hash(initial_snapshot, final_snapshot),
            "changed_files": changed,
            "verifier": verifier,
            "tool_calls": receipt.tool_calls,
            "tool_lifecycle_receipt": receipt.tool_lifecycle_receipt,
            "tool_failures": receipt.tool_failures,
            "patch_calls": receipt.patch_calls,
            "patch_failures": receipt.patch_failures,
            "verification_runs": receipt.verification_runs,
            "verification_failures": receipt.verification_failures,
            "requests": {
                "started": accounting.get("api_request_count"),
                "completed": accounting.get("api_request_completed"),
                "in_flight": accounting.get("api_request_in_flight"),
                "root": {
                    "started": accounting.get("api_request_root_started"),
                    "completed": accounting.get("api_request_root_completed"),
                    "in_flight": accounting.get("api_request_root_in_flight"),
                },
                "child": {
                    "started": accounting.get("api_request_child_started"),
                    "completed": accounting.get("api_request_child_completed"),
                    "in_flight": accounting.get("api_request_child_in_flight"),
                },
                "limit": accounting.get("api_request_limit"),
                "budget_exhausted": accounting.get("api_request_budget_exhausted"),
                "rejected_exhausted": accounting.get("api_request_rejected_exhausted"),
                "rejected_after_seal_observed": accounting.get(
                    "api_request_rejected_after_seal_observed"
                ),
                "transport_retries": accounting.get("transport_retry_count"),
                "actor_accounting_required": actor_accounting_required,
                "actor_accounting_valid": actor_accounting_valid,
            },
            "tokens": {
                "input": accounting.get("input_tokens"),
                "output": accounting.get("output_tokens"),
                "total": accounting.get("total_tokens"),
                "cache_hit": accounting.get("prompt_cache_hit_tokens"),
                "cache_miss": accounting.get("prompt_cache_miss_tokens"),
                "cache_write": accounting.get("prompt_cache_write_tokens"),
                "reasoning": accounting.get("reasoning_tokens"),
                "reasoning_replay": accounting.get("reasoning_replay_tokens"),
                "usage_responses": accounting.get("usage_response_count"),
                "missing_responses": accounting.get("usage_missing_responses"),
                "incomplete_responses": accounting.get("usage_incomplete_responses"),
                "usage_complete": accounting.get("usage_complete"),
                "records_after_seal_observed": accounting.get(
                    "usage_records_after_seal_observed"
                ),
            },
            "cost": {
                "usd": run_cost_usd,
                "source": "harness_official_reprice",
                "runtime_reported_usd": runtime_reported_cost_usd,
                "runtime_reported_cny": accounting.get("cost_cny"),
                "runtime_report_matches": runtime_cost_matches,
                "complete": accounting.get("cost_complete"),
                "billing_unknown_attempts": accounting.get("billing_unknown_attempts"),
                "unpriced_usage_responses": accounting.get("unpriced_usage_responses"),
                "price_snapshot": PRICE_SNAPSHOT,
                "per_run_ceiling_usd": PER_RUN_COST_CEILING_USD,
                "within_ceiling": cost_within_ceiling,
            },
            "surface_counts": {
                "standard_chat": accounting.get("standard_chat_response_count"),
                "strict_chat": accounting.get("strict_chat_response_count"),
                "fim": accounting.get("fim_response_count"),
            },
            "surface_model_usage_buckets": accounting.get(
                "surface_model_usage_buckets"
            ),
            "measurement_contract_passed": measurement_valid,
            "execution_contract_passed": execution_contract_valid,
            "evidence": {
                "git_commit": target.revision
                if len(target.revision) == 40
                and all(character in "0123456789abcdef" for character in target.revision.lower())
                else None,
                "operator_revision": target.revision,
                "workspace_revision_sha256": final_hash,
                "diff_sha256": diff_hash(initial_snapshot, final_snapshot),
                "api_surfaces": [
                    surface
                    for surface, count in (
                        ("standard_chat", accounting.get("standard_chat_response_count")),
                        ("strict_chat", accounting.get("strict_chat_response_count")),
                        ("fim", accounting.get("fim_response_count")),
                    )
                    if isinstance(count, int) and count > 0
                ],
            },
            "metrics": {
                "success": verified_success,
                "false_success": false_success,
                "requests_started": accounting.get("api_request_count"),
                "total_tokens": accounting.get("total_tokens"),
                "wall_time_ms": process.wall_time_ms,
                "cost_usd": run_cost_usd,
                "model_turns": accounting.get("usage_response_count"),
                "patch_failures": receipt.patch_failures,
                "verification_runs": receipt.verification_runs,
                "test_regressions": None,
                "resume_success": None,
                "worktree_conflicts": None,
                "not_applicable": {
                    "test_regressions": "single_fixture_has_no_regression_suite",
                    "resume_success": "resume_not_exercised_by_this_task",
                    "worktree_conflicts": "shared_read_only_explorer_lane_has_no_worktree_merge",
                },
            },
        }


def selected_lanes(value: str) -> list[str]:
    return ["single", "multi"] if value == "all" else [value]


def fixture_sha256() -> str:
    return workspace_hash(snapshot_workspace(FIXTURE_WORKSPACE))


def summarize_numbers(values: list[int | float]) -> dict[str, Any]:
    if not values:
        return {
            "count": 0,
            "total": None,
            "mean": None,
            "median": None,
            "min": None,
            "max": None,
        }
    numeric = [float(value) for value in values]
    return {
        "count": len(numeric),
        "total": round(sum(numeric), 9),
        "mean": round(statistics.fmean(numeric), 9),
        "median": round(float(statistics.median(numeric)), 9),
        "min": round(min(numeric), 9),
        "max": round(max(numeric), 9),
    }


def record_metric(record: dict[str, Any], *path: str) -> int | float | None:
    value: Any = record
    for part in path:
        if not isinstance(value, dict):
            return None
        value = value.get(part)
    return value if isinstance(value, (int, float)) and not isinstance(value, bool) else None


def aggregate_cell(
    target: EvaluationTarget,
    lane: str,
    records: list[dict[str, Any]],
    runs_per_cell: int,
    ab_configured: bool,
) -> dict[str, Any]:
    cell_records = [
        record
        for record in records
        if record.get("variant") == target.variant and record.get("lane") == lane
    ]
    metric_paths = {
        "requests_started": ("requests", "started"),
        "input_tokens": ("tokens", "input"),
        "output_tokens": ("tokens", "output"),
        "total_tokens": ("tokens", "total"),
        "wall_time_ms": ("wall_time_ms",),
        "cost_usd": ("cost", "usd"),
    }
    metric_values = {
        name: [
            value
            for record in cell_records
            if (value := record_metric(record, *path)) is not None
        ]
        for name, path in metric_paths.items()
    }
    repetition_indices = [
        optional_int(record.get("repetition_index")) for record in cell_records
    ]
    complete_repetitions = None not in repetition_indices and sorted(
        int(index) for index in repetition_indices if index is not None
    ) == list(range(1, runs_per_cell + 1))
    measurement_complete = all(
        len(values) == len(cell_records) for values in metric_values.values()
    ) and all(
        record.get("measurement_contract_passed") is True
        and isinstance(record.get("stream_contract"), dict)
        and record["stream_contract"].get("passed") is True
        for record in cell_records
    )
    lane_contract_evidence_complete = all(
        isinstance(record.get("lane_contract"), dict)
        and record["lane_contract"].get("evidence_complete") is True
        for record in cell_records
    )
    outcomes_complete = all(
        isinstance(record.get("verified_success"), bool)
        and isinstance(record.get("false_success"), bool)
        and record.get("status") in {"passed", "failed"}
        and record.get("status")
        == ("passed" if record.get("verified_success") is True else "failed")
        and not (
            record.get("verified_success") is True
            and record.get("false_success") is True
        )
        for record in cell_records
    )
    cost_ceiling_violations = sum(
        isinstance(record.get("cost"), dict)
        and record["cost"].get("within_ceiling") is False
        for record in cell_records
    )
    budget_hashes = {
        record.get("execution_budget_sha256")
        for record in cell_records
        if isinstance(record.get("execution_budget_sha256"), str)
    }
    binary_hashes = {
        record.get("target_binary_sha256")
        for record in cell_records
        if isinstance(record.get("target_binary_sha256"), str)
    }
    runtime_binary_hashes = {
        record.get("runtime_binary_sha256")
        for record in cell_records
        if isinstance(record.get("runtime_binary_sha256"), str)
    }
    binary_pair_hashes = {
        record.get("binary_pair_sha256")
        for record in cell_records
        if isinstance(record.get("binary_pair_sha256"), str)
    }
    tool_catalog_hashes = {
        record.get("tool_catalog_sha256")
        for record in cell_records
        if isinstance(record.get("tool_catalog_sha256"), str)
    }
    prompt_hashes = {
        record.get("prompt_sha256")
        for record in cell_records
        if isinstance(record.get("prompt_sha256"), str)
    }
    budget_stable = len(budget_hashes) == 1 and all(
        isinstance(record.get("execution_budget_sha256"), str)
        for record in cell_records
    )
    target_binary_stable = len(binary_hashes) == 1 and all(
        isinstance(record.get("target_binary_sha256"), str) for record in cell_records
    )
    runtime_binary_stable = len(runtime_binary_hashes) == 1 and all(
        isinstance(record.get("runtime_binary_sha256"), str) for record in cell_records
    )
    binary_pair_stable = len(binary_pair_hashes) == 1 and all(
        isinstance(record.get("binary_pair_sha256"), str) for record in cell_records
    )
    tool_catalog_stable = len(tool_catalog_hashes) == 1 and all(
        valid_sha256(record.get("tool_catalog_sha256")) for record in cell_records
    )
    prompt_stable = len(prompt_hashes) == 1 and all(
        valid_sha256(record.get("prompt_sha256")) for record in cell_records
    )
    revision_stable = all(
        record.get("target_revision") == target.revision for record in cell_records
    )
    eligibility_reasons: list[str] = []
    if not ab_configured:
        eligibility_reasons.append("baseline_and_candidate_required")
    if runs_per_cell < MIN_RUNS_PER_CELL:
        eligibility_reasons.append("minimum_runs_per_cell_not_met")
    if len(cell_records) != runs_per_cell or not complete_repetitions:
        eligibility_reasons.append("cell_runs_incomplete")
    if not measurement_complete:
        eligibility_reasons.append("cell_measurement_incomplete")
    if not lane_contract_evidence_complete:
        eligibility_reasons.append("cell_lane_contract_evidence_incomplete")
    if not outcomes_complete:
        eligibility_reasons.append("cell_outcomes_incomplete")
    if not budget_stable:
        eligibility_reasons.append("cell_budget_not_stable")
    if not target_binary_stable:
        eligibility_reasons.append("cell_binary_not_stable")
    if not runtime_binary_stable:
        eligibility_reasons.append("cell_runtime_binary_not_stable")
    if not binary_pair_stable:
        eligibility_reasons.append("cell_binary_pair_not_stable")
    if not tool_catalog_stable:
        eligibility_reasons.append("cell_tool_catalog_not_stable")
    if not prompt_stable:
        eligibility_reasons.append("cell_prompt_not_stable")
    if not revision_stable:
        eligibility_reasons.append("cell_revision_not_stable")

    successes = sum(record.get("verified_success") is True for record in cell_records)
    false_successes = sum(record.get("false_success") is True for record in cell_records)
    completed = len(cell_records)
    return {
        "record_type": "cell_summary",
        "record_class": "coding_eval",
        "product_metric_eligible": not eligibility_reasons,
        "eligibility_reasons": eligibility_reasons,
        "evidence_complete": (
            len(cell_records) == runs_per_cell
            and complete_repetitions
            and measurement_complete
            and lane_contract_evidence_complete
            and outcomes_complete
            and budget_stable
            and target_binary_stable
            and runtime_binary_stable
            and binary_pair_stable
            and tool_catalog_stable
            and prompt_stable
            and revision_stable
        ),
        "cell_id": f"{target.variant}:{lane}",
        "variant": target.variant,
        "lane": lane,
        "target_revision": target.revision,
        "target_binary": str(target.binary),
        "target_binary_sha256": next(iter(binary_hashes))
        if target_binary_stable
        else None,
        "runtime_binary_sha256": next(iter(runtime_binary_hashes))
        if runtime_binary_stable
        else None,
        "binary_pair_sha256": next(iter(binary_pair_hashes))
        if binary_pair_stable
        else None,
        "tool_catalog_sha256": next(iter(tool_catalog_hashes))
        if tool_catalog_stable
        else None,
        "prompt_sha256": next(iter(prompt_hashes)) if prompt_stable else None,
        "execution_budget_sha256": next(iter(budget_hashes))
        if budget_stable
        else None,
        "runs_planned": runs_per_cell,
        "runs_completed": completed,
        "run_ids": [record.get("run_id") for record in cell_records],
        "task_runs_passed": sum(record.get("status") == "passed" for record in cell_records),
        "cost_ceiling_violations": cost_ceiling_violations,
        "success": {
            "count": successes,
            "rate": successes / completed if completed else 0.0,
        },
        "false_success": {
            "count": false_successes,
            "rate": false_successes / completed if completed else 0.0,
        },
        "metrics": {
            name: summarize_numbers(values) for name, values in metric_values.items()
        },
    }


def candidate_minus_baseline(
    candidate: float | int | None, baseline: float | int | None
) -> dict[str, float | None]:
    if candidate is None or baseline is None:
        return {"absolute": None, "relative": None}
    absolute = float(candidate) - float(baseline)
    relative = absolute / float(baseline) if float(baseline) != 0.0 else None
    return {
        "absolute": round(absolute, 9),
        "relative": round(relative, 9) if relative is not None else None,
    }


def comparison_record(
    lane: str, cell_summaries: list[dict[str, Any]]
) -> dict[str, Any]:
    baseline = next(
        (
            cell
            for cell in cell_summaries
            if cell.get("variant") == "baseline" and cell.get("lane") == lane
        ),
        None,
    )
    candidate = next(
        (
            cell
            for cell in cell_summaries
            if cell.get("variant") == "candidate" and cell.get("lane") == lane
        ),
        None,
    )
    reasons: list[str] = []
    if baseline is None:
        reasons.append("baseline_required")
    if candidate is None:
        reasons.append("candidate_required")
    if baseline is not None and baseline.get("product_metric_eligible") is not True:
        reasons.append("baseline_cell_ineligible")
    if candidate is not None and candidate.get("product_metric_eligible") is not True:
        reasons.append("candidate_cell_ineligible")
    if (
        baseline is not None
        and candidate is not None
        and baseline.get("execution_budget_sha256")
        != candidate.get("execution_budget_sha256")
    ):
        reasons.append("ab_budget_mismatch")
    if (
        baseline is not None
        and candidate is not None
        and baseline.get("target_revision") == candidate.get("target_revision")
    ):
        reasons.append("baseline_candidate_revision_must_differ")
    if (
        baseline is not None
        and candidate is not None
        and baseline.get("binary_pair_sha256") == candidate.get("binary_pair_sha256")
    ):
        reasons.append("baseline_candidate_binary_pair_must_differ")
    tool_catalog_changed = bool(
        baseline is not None
        and candidate is not None
        and baseline.get("tool_catalog_sha256")
        != candidate.get("tool_catalog_sha256")
    )
    prompt_changed = bool(
        baseline is not None
        and candidate is not None
        and baseline.get("prompt_sha256") != candidate.get("prompt_sha256")
    )

    def cell_value(cell: dict[str, Any] | None, *path: str) -> Any:
        value: Any = cell
        for part in path:
            if not isinstance(value, dict):
                return None
            value = value.get(part)
        return value

    delta_paths = {
        "success_rate": ("success", "rate"),
        "false_success_rate": ("false_success", "rate"),
        "requests_started_mean": ("metrics", "requests_started", "mean"),
        "total_tokens_mean": ("metrics", "total_tokens", "mean"),
        "wall_time_ms_mean": ("metrics", "wall_time_ms", "mean"),
        "cost_usd_mean": ("metrics", "cost_usd", "mean"),
    }
    return {
        "record_type": "lane_comparison",
        "record_class": "coding_eval",
        "product_metric_eligible": not reasons,
        "eligibility_reasons": reasons,
        "lane": lane,
        "baseline": baseline,
        "candidate": candidate,
        "treatment": {
            "declaration": (
                "candidate runtime revision, including intentional agent schema, "
                "prompt, and tool-catalog changes"
            ),
            "tool_catalog_changed": tool_catalog_changed,
            "evaluation_prompt_changed": prompt_changed,
        },
        "candidate_minus_baseline": {
            name: candidate_minus_baseline(
                cell_value(candidate, *path), cell_value(baseline, *path)
            )
            for name, path in delta_paths.items()
        },
    }


def pair_variant_order(
    repetition_index: int, lane: str, available_variants: list[str]
) -> list[str]:
    available = set(available_variants)
    if {"baseline", "candidate"}.issubset(available):
        single_baseline_first = repetition_index % 2 == 1
        baseline_first = (
            single_baseline_first if lane == "single" else not single_baseline_first
        )
        ordered = (
            ["baseline", "candidate"]
            if baseline_first
            else ["candidate", "baseline"]
        )
        return [*ordered, *sorted(available - {"baseline", "candidate"})]
    return sorted(available)


def build_run_schedule(
    lanes: list[str], variants: list[str], runs_per_cell: int
) -> list[dict[str, Any]]:
    schedule: list[dict[str, Any]] = []
    schedule_position = 0
    for repetition_index in range(1, runs_per_cell + 1):
        for lane in lanes:
            order = pair_variant_order(repetition_index, lane, variants)
            pair_order = "_".join(order)
            for pair_position, variant in enumerate(order, start=1):
                schedule_position += 1
                schedule.append(
                    {
                        "schedule_position": schedule_position,
                        "pair_position": pair_position,
                        "pair_order": pair_order,
                        "repetition_index": repetition_index,
                        "lane": lane,
                        "variant": variant,
                    }
                )
    return schedule


def schedule_balance(schedule: list[dict[str, Any]]) -> dict[str, Any]:
    pair_heads = [entry for entry in schedule if entry.get("pair_position") == 1]
    baseline_first = sum(
        entry.get("pair_order") == "baseline_candidate" for entry in pair_heads
    )
    candidate_first = sum(
        entry.get("pair_order") == "candidate_baseline" for entry in pair_heads
    )
    return {
        "pair_count": len(pair_heads),
        "baseline_first_pairs": baseline_first,
        "candidate_first_pairs": candidate_first,
        "exactly_balanced": baseline_first == candidate_first and baseline_first > 0,
    }


def plan_record(
    lanes: list[str],
    targets: list[EvaluationTarget],
    model: str,
    runs_per_cell: int,
    configuration_errors: list[str],
) -> dict[str, Any]:
    planned_eligible, eligibility_reasons = planned_eligibility(targets, runs_per_cell)
    eligibility_reasons = [*configuration_errors, *eligibility_reasons]
    schedule = build_run_schedule(
        lanes, [target.variant for target in targets], runs_per_cell
    )
    planned_runs = len(schedule)
    return {
        "record_type": "plan",
        "record_class": "coding_eval",
        # A plan is never evidence. It may only state that the planned matrix
        # would be eligible if every cell completes with valid accounting.
        "product_metric_eligible": False,
        "planned_product_metric_eligible": planned_eligible
        and not configuration_errors,
        "eligibility_reasons": eligibility_reasons,
        "task_id": TASK_ID,
        "lanes": lanes,
        "runs_per_cell": runs_per_cell,
        "minimum_runs_per_cell": MIN_RUNS_PER_CELL,
        "model": model,
        "provider": "deepseek",
        "reasoning_effort": "high",
        "sandbox": "workspace-write",
        "allowed_tools": list(ALLOWED_TOOLS),
        "targets": [target.plan_manifest() for target in targets],
        "automatic_checkout": False,
        "schedule_policy": SCHEDULE_POLICY,
        "schedule_balance": schedule_balance(schedule),
        "schedule": schedule,
        "revision_attestation": "operator_supplied",
        "fixture_sha256": fixture_sha256(),
        "harness_sha256": sha256_file(Path(__file__).resolve()),
        "verifier_sha256": sha256_file(VERIFIER),
        "base_task_sha256": sha256_bytes(BASE_TASK.encode("utf-8")),
        "prompt_sha256": {
            lane: sha256_bytes(lane_prompt(lane).encode("utf-8")) for lane in lanes
        },
        "cells": [
            {
                "cell_id": f"{target.variant}:{lane}",
                "variant": target.variant,
                "lane": lane,
                "revision": target.revision,
                "runs": runs_per_cell,
                "execution_budget": execution_budget(lane, model),
                "execution_budget_sha256": json_hash(execution_budget(lane, model)),
            }
            for lane in lanes
            for target in targets
        ],
        "planned_runs": planned_runs,
        "per_run_cost_ceiling_usd": PER_RUN_COST_CEILING_USD,
        "suite_max_api_requests": planned_runs * MAX_API_REQUESTS_PER_LANE,
        "suite_cost_ceiling_usd": round(
            planned_runs * PER_RUN_COST_CEILING_USD, 9
        ),
        "cost_ceiling_enforcement": "post_run",
        "cost_warning": "费用上限为整套运行后的验收线，不是 API 发送前的实时硬停止线。",
        "key_accessed": False,
        "network_accessed": False,
    }


def run_suite(args: argparse.Namespace) -> int:
    if args.self_test and args.output:
        return fail_preflight("output_not_supported_for_self_test")
    if args.self_test:
        return run_self_tests()
    lanes = selected_lanes(args.lane)
    targets, configuration_errors = configured_targets(args)
    plan = plan_record(
        lanes,
        targets,
        args.model,
        args.runs_per_cell,
        configuration_errors,
    )
    if args.dry_run:
        emit(plan)
        return 0
    if configuration_errors:
        return fail_preflight("target_configuration_invalid")
    if target_by_variant(targets, "candidate") is None:
        return fail_preflight("candidate_target_required")
    ab_configured = (
        target_by_variant(targets, "baseline") is not None
        and target_by_variant(targets, "candidate") is not None
    )
    planned_eligible, _ = planned_eligibility(targets, args.runs_per_cell)
    if ab_configured and not planned_eligible:
        return fail_preflight("ab_target_identity_or_matrix_invalid")
    if not args.acknowledge_cost:
        return fail_preflight("cost_acknowledgement_required")
    if not args.key_file:
        return fail_preflight("key_file_required")
    for target in targets:
        identity = binary_pair_identity(target.binary)
        if not identity["launcher_executable"] or not identity["runtime_executable"]:
            return fail_preflight(f"{target.variant}_binary_pair_unavailable")
    if not shutil.which("git"):
        return fail_preflight("git_unavailable")
    evaluation_id = "deepseek-exec-ab-" + uuid.uuid4().hex
    frozen_bundle = tempfile.TemporaryDirectory(prefix="codewhale-eval-binaries-")
    try:
        frozen_targets = [
            freeze_target(target, Path(frozen_bundle.name)) for target in targets
        ]
        frozen_assets = freeze_evaluation_assets(Path(frozen_bundle.name))
    except (OSError, ValueError):
        frozen_bundle.cleanup()
        return fail_preflight("binary_pair_freeze_failed")
    if (
        ab_configured
        and len({frozen.pair_sha256 for frozen in frozen_targets}) != len(frozen_targets)
    ):
        frozen_bundle.cleanup()
        return fail_preflight("baseline_candidate_frozen_pair_must_differ")
    execution_schedule = build_run_schedule(
        lanes, [target.variant for target in targets], args.runs_per_cell
    )
    planned_run_count = len(execution_schedule)
    execution_schedule_balance = schedule_balance(execution_schedule)
    suite_cost_ceiling = round(
        planned_run_count * PER_RUN_COST_CEILING_USD, 9
    )
    common = {
        "record_class": "coding_eval",
        "product_metric_eligible": False,
        "evaluation_id": evaluation_id,
        "task_id": TASK_ID,
        "model": args.model,
        "provider": "deepseek",
        "reasoning_effort": "high",
        "sandbox": "workspace-write",
        "allowed_tools": list(ALLOWED_TOOLS),
        "automatic_checkout": False,
        "schedule_policy": SCHEDULE_POLICY,
        "schedule_balance": execution_schedule_balance,
        "credential_contract": {
            "key_source": "key_file",
            "child_env_only": True,
            "key_in_argv": False,
            "key_in_results": False,
        },
        "fixture_sha256": frozen_assets.fixture_sha256,
        "harness_sha256": HARNESS_SOURCE_SHA256,
        "verifier_sha256": frozen_assets.verifier_sha256,
        "base_task_sha256": sha256_bytes(BASE_TASK.encode("utf-8")),
        "runs_per_cell": args.runs_per_cell,
        "minimum_runs_per_cell": MIN_RUNS_PER_CELL,
        "per_run_cost_ceiling_usd": PER_RUN_COST_CEILING_USD,
        "suite_cost_ceiling_usd": suite_cost_ceiling,
        "targets": {
            frozen.source.variant: {
                "revision": frozen.source.revision,
                "binary": str(frozen.source.binary),
                "frozen_binary": str(frozen.execution.binary),
                "binary_sha256": frozen.launcher_sha256,
                "runtime_binary_sha256": frozen.runtime_sha256,
                "binary_pair_sha256": frozen.pair_sha256,
                "revision_attestation": "operator_supplied",
            }
            for frozen in frozen_targets
        },
        **git_metadata(),
    }
    suite_started = time.monotonic()
    results: list[dict[str, Any]] = []
    frozen_by_variant = {
        frozen.source.variant: frozen for frozen in frozen_targets
    }
    try:
        key = load_key(Path(args.key_file))
    except (OSError, ValueError) as error:
        frozen_bundle.cleanup()
        code = str(error) if isinstance(error, ValueError) else "key_unreadable"
        return fail_preflight(code, key_accessed=True)
    try:
        for schedule_entry in execution_schedule:
            frozen = frozen_by_variant[str(schedule_entry["variant"])]
            result = run_lane(
                str(schedule_entry["lane"]),
                frozen,
                frozen_assets,
                int(schedule_entry["repetition_index"]),
                str(schedule_entry["pair_order"]),
                int(schedule_entry["pair_position"]),
                int(schedule_entry["schedule_position"]),
                args.model,
                key,
                common,
            )
            results.append(result)
            emit(result)
            spent = sum(
                float(record["cost"]["usd"])
                for record in results
                if record["cost"]["usd"] is not None
            )
            if spent > suite_cost_ceiling or result["cost"]["within_ceiling"] is False:
                break
    finally:
        key = ""
        frozen_bundle.cleanup()

    cell_summaries = [
        aggregate_cell(target, lane, results, args.runs_per_cell, ab_configured)
        for lane in lanes
        for target in targets
    ]
    for cell in cell_summaries:
        emit({**common, **cell})
    comparisons = [comparison_record(lane, cell_summaries) for lane in lanes]
    for comparison in comparisons:
        emit({**common, **comparison})

    costs = [record["cost"]["usd"] for record in results]
    cost_known = len(costs) == planned_run_count and all(
        value is not None for value in costs
    )
    total_cost = sum(float(value) for value in costs if value is not None)
    evidence_complete = (
        len(results) == planned_run_count
        and all(cell.get("evidence_complete") is True for cell in cell_summaries)
        and cost_known
        and total_cost <= suite_cost_ceiling
    )
    product_metric_eligible = bool(
        comparisons
        and all(
            comparison.get("product_metric_eligible") is True
            for comparison in comparisons
        )
    )
    summary_eligibility_reasons = sorted(
        {
            reason
            for comparison in comparisons
            for reason in comparison.get("eligibility_reasons", [])
            if isinstance(reason, str)
        }
    )
    total_requests = summarize_numbers(
        [
            value
            for record in results
            if (value := record_metric(record, "requests", "started")) is not None
        ]
    )
    total_tokens = summarize_numbers(
        [
            value
            for record in results
            if (value := record_metric(record, "tokens", "total")) is not None
        ]
    )
    wall_times = summarize_numbers(
        [
            value
            for record in results
            if (value := record_metric(record, "wall_time_ms")) is not None
        ]
    )
    emit(
        {
            **common,
            "record_type": "evaluation_summary",
            "product_metric_eligible": product_metric_eligible,
            "eligibility_reasons": summary_eligibility_reasons,
            "evidence_complete": evidence_complete,
            "status": "completed" if evidence_complete else "incomplete",
            "lanes_planned": lanes,
            "runs_planned": planned_run_count,
            "runs_completed": len(results),
            "task_runs_passed": sum(record["status"] == "passed" for record in results),
            "verified_successes": sum(record["verified_success"] for record in results),
            "false_successes": sum(record["false_success"] for record in results),
            "cell_ids": [cell["cell_id"] for cell in cell_summaries],
            "comparison_lanes": [comparison["lane"] for comparison in comparisons],
            "suite_wall_time_ms": int((time.monotonic() - suite_started) * 1000),
            "requests_started": total_requests,
            "total_tokens": total_tokens,
            "run_wall_time_ms": wall_times,
            "suite_max_api_requests": planned_run_count * MAX_API_REQUESTS_PER_LANE,
            "estimated_cost_usd": round(total_cost, 9),
            "cost_known": cost_known,
            "cost_within_ceiling": cost_known and total_cost <= suite_cost_ceiling,
            "suite_cost_ceiling_usd": suite_cost_ceiling,
            "cost_ceiling_enforcement": "post_run",
            "price_snapshot": PRICE_SNAPSHOT,
        }
    )
    return 0 if evidence_complete else 1


def self_test_target(
    root: Path, variant: str, revision: str, marker: str
) -> EvaluationTarget:
    directory = root / variant
    directory.mkdir(parents=True)
    launcher = directory / "codewhale"
    runtime = directory / "codewhale-tui"
    launcher.write_text(f"#!/bin/sh\n# {marker}-launcher\nexit 0\n", encoding="utf-8")
    runtime.write_text(f"#!/bin/sh\n# {marker}-runtime\nexit 0\n", encoding="utf-8")
    launcher.chmod(0o755)
    runtime.chmod(0o755)
    return EvaluationTarget(variant, launcher, revision)


def aggregate_fixture_records() -> list[dict[str, Any]]:
    records = json.loads(AGGREGATE_FIXTURE.read_text(encoding="utf-8"))
    for record in records:
        record["measurement_contract_passed"] = record.pop(
            "accounting_contract_passed"
        )
        record.setdefault("lane_contract", {})["evidence_complete"] = True
        record["binary_pair_sha256"] = sha256_bytes(
            str(record["variant"]).encode("utf-8")
        )
        record["tool_catalog_sha256"] = sha256_bytes(b"fixed-tool-catalog")
        record["prompt_sha256"] = sha256_bytes(
            lane_prompt(str(record["lane"])).encode("utf-8")
        )
    return records


def stream_event(event_type: str, **fields: Any) -> dict[str, Any]:
    return {
        "schema": "codewhale.exec-stream",
        "schema_version": 1,
        "type": event_type,
        **fields,
    }


class HarnessSelfTests(unittest.TestCase):
    def test_actor_request_accounting_is_exact_when_present(self) -> None:
        terminal = {
            "api_request_count": 10,
            "api_request_completed": 10,
            "api_request_in_flight": 0,
            "api_request_root_started": 8,
            "api_request_root_completed": 8,
            "api_request_root_in_flight": 0,
            "api_request_child_started": 2,
            "api_request_child_completed": 2,
            "api_request_child_in_flight": 0,
        }
        self.assertTrue(actor_request_accounting_valid(terminal))
        self.assertTrue(
            actor_request_accounting_valid(terminal, required=True, lane="multi")
        )
        self.assertFalse(
            actor_request_accounting_valid(terminal, required=True, lane="single")
        )
        terminal["api_request_child_started"] = 1
        self.assertFalse(actor_request_accounting_valid(terminal))
        terminal.pop("api_request_child_started")
        self.assertFalse(actor_request_accounting_valid(terminal))
        self.assertTrue(
            actor_request_accounting_valid(
                {
                    "api_request_count": 1,
                    "api_request_completed": 1,
                    "api_request_in_flight": 0,
                }
            ),
            "frozen historical baselines without actor fields remain measurable",
        )
        self.assertFalse(
            actor_request_accounting_valid(
                {
                    "api_request_count": 1,
                    "api_request_completed": 1,
                    "api_request_in_flight": 0,
                },
                required=True,
                lane="multi",
            ),
            "candidate evidence must never pass without actor fields",
        )

        single = {
            "api_request_count": 3,
            "api_request_completed": 3,
            "api_request_in_flight": 0,
            "api_request_root_started": 3,
            "api_request_root_completed": 3,
            "api_request_root_in_flight": 0,
            "api_request_child_started": 0,
            "api_request_child_completed": 0,
            "api_request_child_in_flight": 0,
        }
        self.assertTrue(
            actor_request_accounting_valid(single, required=True, lane="single")
        )
        self.assertFalse(
            actor_request_accounting_valid(single, required=True, lane="multi")
        )

        cross_actor_mismatch = dict(terminal)
        cross_actor_mismatch.update(
            {
                "api_request_child_started": 2,
                "api_request_root_completed": 7,
                "api_request_child_completed": 3,
            }
        )
        self.assertTrue(actor_request_accounting_valid(cross_actor_mismatch))
        self.assertFalse(
            actor_request_accounting_valid(
                cross_actor_mismatch, required=True, lane="multi"
            ),
            "candidate actors must each settle their own leases",
        )

    def test_fixture_starts_failing_and_fixed_version_passes(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            workspace = Path(temporary) / "workspace"
            initial = initialize_fixture_workspace(workspace)
            self.assertFalse(run_verifier(workspace)["passed"])
            source = (workspace / "ranges.py").read_text(encoding="utf-8")
            (workspace / "ranges.py").write_text(
                source.replace("start = previous\n", "start = current\n"), encoding="utf-8"
            )
            self.assertTrue(run_verifier(workspace)["passed"])
            final = snapshot_workspace(workspace)
            self.assertEqual(changed_files(initial, final), EXPECTED_CHANGED_FILES)

    def test_runtime_state_is_not_user_code_but_other_workspace_files_are(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            workspace = Path(temporary) / "workspace"
            initial = initialize_fixture_workspace(workspace)
            source = (workspace / "ranges.py").read_text(encoding="utf-8")
            (workspace / "ranges.py").write_text(
                source.replace("start = previous\n", "start = current\n"), encoding="utf-8"
            )
            runtime_state = workspace / ".codewhale" / "state" / "run.json"
            runtime_state.parent.mkdir(parents=True)
            runtime_state.write_text('{"terminal":"completed"}', encoding="utf-8")

            final = snapshot_workspace(workspace)
            self.assertNotIn(".codewhale/state/run.json", final)
            self.assertEqual(changed_files(initial, final), EXPECTED_CHANGED_FILES)
            self.assertTrue(run_verifier(workspace)["passed"])

            user_file = workspace / ".codewhale" / "notes.txt"
            user_file.write_text("user-owned", encoding="utf-8")
            with_user_file = snapshot_workspace(workspace)
            self.assertIn(".codewhale/notes.txt", with_user_file)
            self.assertFalse(run_verifier(workspace)["passed"])

    def test_command_never_contains_a_key(self) -> None:
        marker = "sk-self-test-do-not-leak"
        command = exec_command(Path("/tmp/codewhale"), "single", DEFAULT_MODEL)
        self.assertNotIn(marker, command)
        self.assertNotIn("DEEPSEEK_API_KEY", command)
        self.assertLess(command.index("--provider"), command.index("exec"))
        with tempfile.TemporaryDirectory() as temporary:
            poison = {
                "DEEPSEEK_TUI_BIN": "/tmp/poison-runtime",
                "CODEWHALE_CONFIG": "/tmp/poison-config",
                "PYTHONPATH": "/tmp/poison-python",
                "GIT_CONFIG_GLOBAL": "/tmp/poison-gitconfig",
                "DYLD_INSERT_LIBRARIES": "/tmp/poison.dylib",
            }
            previous = {name: os.environ.get(name) for name in poison}
            try:
                os.environ.update(poison)
                environment = child_environment(marker, Path(temporary))
                self.assertEqual(environment["DEEPSEEK_API_KEY"], marker)
                self.assertNotIn("DEEPSEEK_TUI_BIN", environment)
                self.assertNotIn("CODEWHALE_CONFIG", environment)
                host_environment = sanitized_host_environment()
                for name in poison:
                    self.assertNotIn(name, environment)
                    self.assertNotIn(name, host_environment)
                for name in ("HTTPS_PROXY", "HTTP_PROXY", "ALL_PROXY"):
                    self.assertNotIn(name, host_environment)
                self.assertEqual(host_environment["GIT_CONFIG_NOSYSTEM"], "1")
            finally:
                for name, value in previous.items():
                    if value is None:
                        os.environ.pop(name, None)
                    else:
                        os.environ[name] = value

    def test_plan_is_not_evidence_and_requires_explicit_ab(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            baseline = self_test_target(
                root, "baseline", "baseline-revision", "baseline"
            )
            candidate = self_test_target(
                root, "candidate", "candidate-revision", "candidate"
            )
            plan = plan_record(
                ["single", "multi"],
                [baseline, candidate],
                DEFAULT_MODEL,
                MIN_RUNS_PER_CELL,
                [],
            )
            self.assertFalse(plan["product_metric_eligible"])
            self.assertTrue(plan["planned_product_metric_eligible"])
            self.assertFalse(plan["automatic_checkout"])
            self.assertEqual(plan["planned_runs"], 12)
            self.assertEqual(plan["schedule_policy"], SCHEDULE_POLICY)
            self.assertEqual(
                plan["schedule_balance"],
                {
                    "pair_count": 6,
                    "baseline_first_pairs": 3,
                    "candidate_first_pairs": 3,
                    "exactly_balanced": True,
                },
            )
            schedule = plan["schedule"]
            self.assertEqual(
                [entry["schedule_position"] for entry in schedule],
                list(range(1, 13)),
            )
            pair_heads = [
                entry for entry in schedule if entry["pair_position"] == 1
            ]
            self.assertEqual(
                [
                    (entry["repetition_index"], entry["lane"], entry["variant"])
                    for entry in pair_heads
                ],
                [
                    (1, "single", "baseline"),
                    (1, "multi", "candidate"),
                    (2, "single", "candidate"),
                    (2, "multi", "baseline"),
                    (3, "single", "baseline"),
                    (3, "multi", "candidate"),
                ],
            )
            self.assertEqual(
                build_run_schedule(
                    ["single", "multi"],
                    ["candidate", "baseline"],
                    MIN_RUNS_PER_CELL,
                ),
                schedule,
            )

            no_baseline = plan_record(
                ["single"], [candidate], DEFAULT_MODEL, MIN_RUNS_PER_CELL, []
            )
            self.assertFalse(no_baseline["planned_product_metric_eligible"])
            self.assertIn("baseline_required", no_baseline["eligibility_reasons"])

            too_few = plan_record(
                ["single"],
                [baseline, candidate],
                DEFAULT_MODEL,
                MIN_RUNS_PER_CELL - 1,
                [],
            )
            self.assertFalse(too_few["planned_product_metric_eligible"])
            self.assertIn(
                "minimum_runs_per_cell_not_met", too_few["eligibility_reasons"]
            )

            same_revision = plan_record(
                ["single"],
                [baseline, dataclasses.replace(candidate, revision=baseline.revision)],
                DEFAULT_MODEL,
                MIN_RUNS_PER_CELL,
                [],
            )
            self.assertFalse(same_revision["planned_product_metric_eligible"])
            self.assertIn(
                "baseline_candidate_revision_must_differ",
                same_revision["eligibility_reasons"],
            )

            same_pair_dir = root / "same-pair"
            shutil.copytree(baseline.binary.parent, same_pair_dir)
            same_pair = EvaluationTarget(
                "candidate", same_pair_dir / "codewhale", "different-label"
            )
            eligible, reasons = planned_eligibility(
                [baseline, same_pair], MIN_RUNS_PER_CELL
            )
            self.assertFalse(eligible)
            self.assertIn("baseline_candidate_binary_pair_must_differ", reasons)

    def test_target_configuration_never_infers_or_checks_out_a_revision(self) -> None:
        args = argparse.Namespace(
            baseline_binary="/tmp/baseline/codewhale",
            baseline_revision=None,
            candidate_binary="/tmp/candidate/codewhale",
            candidate_revision="candidate-revision",
        )
        targets, errors = configured_targets(args)
        self.assertEqual(errors, ["baseline_target_incomplete"])
        self.assertEqual([target.variant for target in targets], ["candidate"])
        self.assertEqual(targets[0].revision, "candidate-revision")

    def test_cell_and_lane_ab_aggregation(self) -> None:
        records = aggregate_fixture_records()
        baseline_target = EvaluationTarget(
            "baseline", Path("/fixture/baseline/codewhale"), "base-revision"
        )
        candidate_target = EvaluationTarget(
            "candidate", Path("/fixture/candidate/codewhale"), "candidate-revision"
        )
        baseline = aggregate_cell(
            baseline_target, "single", records, MIN_RUNS_PER_CELL, True
        )
        candidate = aggregate_cell(
            candidate_target, "single", records, MIN_RUNS_PER_CELL, True
        )
        self.assertTrue(baseline["product_metric_eligible"])
        self.assertTrue(candidate["product_metric_eligible"])
        self.assertEqual(baseline["success"]["count"], 2)
        self.assertEqual(baseline["false_success"]["count"], 1)
        self.assertEqual(candidate["success"]["count"], 3)
        self.assertEqual(candidate["metrics"]["requests_started"]["total"], 8.0)

        comparison = comparison_record("single", [baseline, candidate])
        self.assertTrue(comparison["product_metric_eligible"])
        self.assertAlmostEqual(
            comparison["candidate_minus_baseline"]["success_rate"]["absolute"],
            1 / 3,
            places=9,
        )
        self.assertLess(
            comparison["candidate_minus_baseline"]["total_tokens_mean"]["absolute"],
            0,
        )
        changed_catalog_candidate = json.loads(json.dumps(candidate))
        changed_catalog_candidate["tool_catalog_sha256"] = sha256_bytes(
            b"intentional-candidate-catalog"
        )
        changed_catalog_comparison = comparison_record(
            "single", [baseline, changed_catalog_candidate]
        )
        self.assertTrue(changed_catalog_comparison["product_metric_eligible"])
        self.assertTrue(
            changed_catalog_comparison["treatment"]["tool_catalog_changed"]
        )

        mixed_records = json.loads(json.dumps(records))
        candidate_records = [
            record
            for record in mixed_records
            if record["variant"] == "candidate" and record["lane"] == "single"
        ]
        for record in candidate_records[1:]:
            record["verified_success"] = False
            record["false_success"] = False
            record["status"] = "failed"
            record["lane_contract"]["passed"] = False
        candidate_records[1]["cost"]["within_ceiling"] = False
        mixed = aggregate_cell(
            candidate_target,
            "single",
            mixed_records,
            MIN_RUNS_PER_CELL,
            True,
        )
        self.assertTrue(mixed["product_metric_eligible"])
        self.assertEqual(mixed["success"]["count"], 1)
        self.assertEqual(mixed["cost_ceiling_violations"], 1)

        missing_accounting = json.loads(json.dumps(mixed_records))
        next(
            record
            for record in missing_accounting
            if record["variant"] == "candidate" and record["lane"] == "single"
        )["measurement_contract_passed"] = False
        incomplete = aggregate_cell(
            candidate_target,
            "single",
            missing_accounting,
            MIN_RUNS_PER_CELL,
            True,
        )
        self.assertFalse(incomplete["product_metric_eligible"])
        self.assertIn(
            "cell_measurement_incomplete", incomplete["eligibility_reasons"]
        )

        same_revision_candidate = json.loads(json.dumps(candidate))
        same_revision_candidate["target_revision"] = baseline["target_revision"]
        same_revision_comparison = comparison_record(
            "single", [baseline, same_revision_candidate]
        )
        self.assertFalse(same_revision_comparison["product_metric_eligible"])
        self.assertIn(
            "baseline_candidate_revision_must_differ",
            same_revision_comparison["eligibility_reasons"],
        )

    def test_aggregate_is_ineligible_without_baseline_or_three_runs(self) -> None:
        records = aggregate_fixture_records()
        candidate_target = EvaluationTarget(
            "candidate", Path("/fixture/candidate/codewhale"), "candidate-revision"
        )
        candidate_only = aggregate_cell(
            candidate_target, "single", records, MIN_RUNS_PER_CELL, False
        )
        self.assertFalse(candidate_only["product_metric_eligible"])
        self.assertIn(
            "baseline_and_candidate_required", candidate_only["eligibility_reasons"]
        )
        comparison = comparison_record("single", [candidate_only])
        self.assertFalse(comparison["product_metric_eligible"])
        self.assertIn("baseline_required", comparison["eligibility_reasons"])

        two_runs = [
            record
            for record in records
            if record["variant"] == "candidate" and record["repetition_index"] <= 2
        ]
        insufficient = aggregate_cell(
            candidate_target, "single", two_runs, MIN_RUNS_PER_CELL - 1, True
        )
        self.assertFalse(insufficient["product_metric_eligible"])
        self.assertIn(
            "minimum_runs_per_cell_not_met", insufficient["eligibility_reasons"]
        )

        missing_runtime_receipt = json.loads(json.dumps(records))
        missing_runtime_receipt[3].pop("runtime_binary_sha256")
        broken = aggregate_cell(
            candidate_target,
            "single",
            missing_runtime_receipt,
            MIN_RUNS_PER_CELL,
            True,
        )
        self.assertFalse(broken["product_metric_eligible"])
        self.assertIn(
            "cell_runtime_binary_not_stable", broken["eligibility_reasons"]
        )

        broken_lane_contract = json.loads(json.dumps(records))
        broken_lane_contract[3]["lane_contract"]["evidence_complete"] = False
        broken = aggregate_cell(
            candidate_target,
            "single",
            broken_lane_contract,
            MIN_RUNS_PER_CELL,
            True,
        )
        self.assertFalse(broken["product_metric_eligible"])
        self.assertIn(
            "cell_lane_contract_evidence_incomplete", broken["eligibility_reasons"]
        )

        inconsistent_outcome = json.loads(json.dumps(records))
        inconsistent_outcome[3]["status"] = "failed"
        broken = aggregate_cell(
            candidate_target,
            "single",
            inconsistent_outcome,
            MIN_RUNS_PER_CELL,
            True,
        )
        self.assertFalse(broken["product_metric_eligible"])
        self.assertIn("cell_outcomes_incomplete", broken["eligibility_reasons"])

    def test_stream_contract_and_multi_handoff(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            workspace = Path(temporary)
            (workspace / "ranges.py").write_text("before", encoding="utf-8")
            initial = snapshot_workspace(workspace)
            receipt = StreamReceipt()
            events = [
                stream_event(
                    "tool_use",
                    id="a",
                    name="agent",
                    input={
                        "prompt": "只读检查范围合并缺陷",
                        "type": "explore",
                        "fork_context": False,
                        "expected_artifact": "缺陷诊断",
                        "max_steps": 4,
                        "wall_time_secs": 120,
                        "allowed_tools": ["read_file", "list_dir", "grep_files"],
                    },
                ),
                stream_event(
                    "child_started",
                    call_id="a",
                    child_run_id="child-1",
                    depth=1,
                    started_at="2026-07-18T00:00:00Z",
                ),
                stream_event(
                    "tool_result",
                    id="a",
                    name="agent",
                    status="success",
                    output=json.dumps({"agent_id": "child-1"}),
                ),
                stream_event(
                    "child_finished",
                    call_id="a",
                    child_run_id="child-1",
                    status="completed",
                    result_present=True,
                    completed_at="2026-07-18T00:00:01Z",
                ),
                stream_event(
                    "tool_use",
                    id="p",
                    name="apply_patch",
                    input={"patch": "sk-self-test-do-not-leak"},
                ),
                stream_event(
                    "tool_result",
                    id="p",
                    name="apply_patch",
                    status="success",
                    side_effect_status="applied",
                    output="ok",
                ),
                stream_event(
                    "metadata", meta={"receipt_kind": "terminal", "status": "completed"}
                ),
                stream_event("done"),
            ]
            for item in events:
                receipt.process(item, workspace)
            self.assertEqual(receipt.protocol_errors(), [])
            self.assertTrue(lane_contract("multi", receipt, initial)["passed"])
            self.assertEqual(
                [
                    (entry["name"], entry["status"])
                    for entry in receipt.tool_lifecycle_receipt
                ],
                [
                    ("agent", "success"),
                    ("apply_patch", "success"),
                ],
            )
            self.assertNotIn(
                "sk-self-test-do-not-leak", json.dumps(dataclasses.asdict(receipt))
            )

            missing_result = dataclasses.replace(
                receipt, completed_child_result_proven=False
            )
            blocked = lane_contract("multi", missing_result, initial)
            self.assertFalse(blocked["passed"])
            self.assertTrue(blocked["evidence_complete"])
            self.assertIn("child_result_receipt_missing", blocked["reasons"])
            self.assertEqual(
                classify_claim_outcome(True, blocked["passed"]),
                (False, True),
            )

            no_applied_effect = StreamReceipt()
            for item in events:
                if item.get("type") == "tool_result" and item.get("name") == "apply_patch":
                    item = {**item, "side_effect_status": "not_applied"}
                no_applied_effect.process(item, workspace)
            no_effect_contract = lane_contract("multi", no_applied_effect, initial)
            self.assertFalse(no_effect_contract["passed"])
            self.assertIn(
                "root_mutation_after_handoff_required", no_effect_contract["reasons"]
            )

    def test_legacy_agent_actions_are_neither_spawns_nor_settled_handoffs(self) -> None:
        receipt = StreamReceipt()

        def event(event_type: str, **fields: Any) -> dict[str, Any]:
            return {
                "schema": "codewhale.exec-stream",
                "schema_version": 1,
                "type": event_type,
                **fields,
            }

        receipt.process(
            event("tool_use", id="legacy-wait", name="agent", input={"action": "wait"})
        )
        receipt.process(
            event("tool_result", id="legacy-wait", name="agent", status="success")
        )
        receipt.process(event("tool_result", id="unknown", name="agent", status="success"))

        self.assertEqual(receipt.agent_spawn_count, 0)
        self.assertEqual(receipt.agent_spawn_success_count, 0)
        self.assertEqual(receipt.agent_tool_call_count, 1)
        self.assertEqual(receipt.child_started_count, 0)
        self.assertEqual(receipt.child_finished_count, 0)
        self.assertFalse(lane_contract("multi", receipt)["passed"])

    def test_multi_rejects_extra_agent_and_misordered_or_unlinked_child(self) -> None:
        valid_input = {
            "prompt": "只读检查范围合并缺陷",
            "type": "explore",
            "fork_context": False,
            "expected_artifact": "缺陷诊断",
            "max_steps": 4,
            "wall_time_secs": 120,
            "allowed_tools": ["read_file", "list_dir", "grep_files"],
        }
        with tempfile.TemporaryDirectory() as temporary:
            workspace = Path(temporary)
            (workspace / "ranges.py").write_text("before", encoding="utf-8")
            initial = snapshot_workspace(workspace)
            receipt = StreamReceipt()
            for item in (
                stream_event("tool_use", id="a", name="agent", input=valid_input),
                stream_event(
                    "tool_use",
                    id="legacy",
                    name="agent",
                    input={"action": "wait"},
                ),
                stream_event(
                    "tool_result",
                    id="legacy",
                    name="agent",
                    status="success",
                    output="{}",
                ),
                stream_event(
                    "child_finished",
                    call_id="wrong-call",
                    child_run_id="child-1",
                    status="completed",
                    result_present=True,
                    completed_at="2026-07-18T00:00:01Z",
                ),
                stream_event(
                    "child_started",
                    call_id="wrong-call",
                    child_run_id="child-1",
                    depth=2,
                    started_at="2026-07-18T00:00:02Z",
                ),
                stream_event(
                    "tool_result",
                    id="a",
                    name="agent",
                    status="success",
                    output=json.dumps({"agent_id": "child-1"}),
                ),
            ):
                receipt.process(item, workspace)

            self.assertIn(
                "child_lifecycle_out_of_order", receipt.protocol_errors()
            )
            contract = lane_contract("multi", receipt, initial)
            self.assertFalse(contract["passed"])
            self.assertIn("exactly_one_agent_tool_call_required", contract["reasons"])
            self.assertIn("spawn_child_receipt_mismatch", contract["reasons"])
            self.assertIn("child_lifecycle_order_invalid", contract["reasons"])
            self.assertIn("root_child_depth_must_be_one", contract["reasons"])
            self.assertIn("root_tool_before_handoff", contract["reasons"])

    def test_single_lane_rejects_child_spawn(self) -> None:
        receipt = StreamReceipt(agent_spawn_count=1)
        self.assertFalse(lane_contract("single", receipt)["passed"])

    def test_multi_lane_treats_shell_as_write_capable_before_handoff(self) -> None:
        receipt = StreamReceipt(
            agent_tool_call_count=1,
            agent_spawn_count=1,
            valid_explorer_spawn_count=1,
            agent_spawn_success_count=1,
            child_started_count=1,
            child_finished_count=1,
            first_child_finished_index=5,
            started_children=[("a", "child-1")],
            started_child_indices=[2],
            started_child_depths=[1],
            finished_children=[("a", "child-1")],
            finished_child_indices=[5],
            child_finished_statuses=["completed"],
            completed_child_result_proven=True,
            write_indices=[3, 6],
        )
        contract = lane_contract("multi", receipt)
        self.assertFalse(contract["passed"])
        self.assertEqual(contract["writes_before_handoff"], 1)

    def test_workspace_contract_detects_nested_git_modes_and_symlinks(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            workspace = Path(temporary) / "workspace"
            initialize_fixture_workspace(workspace)
            source = (workspace / "ranges.py").read_text(encoding="utf-8")
            (workspace / "ranges.py").write_text(
                source.replace("start = previous\n", "start = current\n"),
                encoding="utf-8",
            )
            clean = snapshot_workspace(workspace)
            self.assertTrue(run_verifier(workspace)["passed"])

            nested_git = workspace / "vendor" / ".git" / "config"
            nested_git.parent.mkdir(parents=True)
            nested_git.write_text("not root metadata", encoding="utf-8")
            self.assertIn("vendor/.git/config", snapshot_workspace(workspace))
            self.assertFalse(run_verifier(workspace)["passed"])
            shutil.rmtree(workspace / "vendor")

            (workspace / "ranges.py").chmod(0o755)
            executable = snapshot_workspace(workspace)
            self.assertNotEqual(clean["ranges.py"], executable["ranges.py"])
            self.assertFalse(run_verifier(workspace)["passed"])
            (workspace / "ranges.py").chmod(0o644)

            (workspace / "ranges.py").unlink()
            (workspace / "ranges.py").symlink_to("README.md")
            self.assertEqual(snapshot_workspace(workspace)["ranges.py"]["kind"], "symlink")
            self.assertFalse(run_verifier(workspace)["passed"])

    def test_frozen_binary_pair_does_not_follow_source_changes(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            target = self_test_target(root, "candidate", "rev", "before")
            frozen = freeze_target(target, root / "frozen")
            frozen_launcher = frozen.execution.binary.read_bytes()
            frozen_runtime = sibling_runtime_binary(frozen.execution.binary).read_bytes()
            target.binary.write_text("mutated launcher", encoding="utf-8")
            sibling_runtime_binary(target.binary).write_text(
                "mutated runtime", encoding="utf-8"
            )
            self.assertEqual(frozen.execution.binary.read_bytes(), frozen_launcher)
            self.assertEqual(
                sibling_runtime_binary(frozen.execution.binary).read_bytes(), frozen_runtime
            )
            self.assertEqual(binary_pair_identity(frozen.execution.binary)["pair_sha256"], frozen.pair_sha256)

    def test_tool_lifecycle_rejects_orphans_duplicates_and_open_calls(self) -> None:
        receipt = StreamReceipt()
        receipt.process(
            stream_event("tool_result", id="orphan", name="read_file", status="success")
        )
        receipt.process(stream_event("tool_use", id="dup", name="read_file", input={}))
        receipt.process(stream_event("tool_use", id="dup", name="grep_files", input={}))
        errors = receipt.protocol_errors()
        self.assertIn("orphan_tool_result", errors)
        self.assertIn("duplicate_tool_id", errors)
        self.assertIn("tool_result_missing", errors)

        after_terminal = StreamReceipt()
        after_terminal.process(
            stream_event(
                "metadata", meta={"receipt_kind": "terminal", "status": "completed"}
            )
        )
        after_terminal.process(stream_event("content", content="late"))
        after_terminal.process(stream_event("done"))
        self.assertIn(
            "event_after_terminal_metadata", after_terminal.protocol_errors()
        )

        failed_stream = StreamReceipt()
        failed_stream.process(
            stream_event(
                "metadata",
                meta={
                    "receipt_kind": "terminal",
                    "status": "failed",
                    "termination_reason": "model_error",
                },
            )
        )
        failed_stream.process(
            stream_event(
                "error",
                code="model_error",
                category="model",
                recoverable=False,
                error="redacted",
                termination_reason="model_error",
            )
        )
        failed_stream.process(stream_event("done"))
        self.assertEqual(failed_stream.protocol_errors(), [])
        self.assertEqual(failed_stream.error_count, 1)

    def test_typed_failure_envelope_is_exact(self) -> None:
        def terminal(status: str, reason: str) -> dict[str, Any]:
            return stream_event(
                "metadata",
                meta={
                    "receipt_kind": "terminal",
                    "status": status,
                    "termination_reason": reason,
                },
            )

        def error(reason: str) -> dict[str, Any]:
            return stream_event(
                "error",
                code="exec_turn_failed",
                category="runtime",
                recoverable=False,
                error="redacted",
                termination_reason=reason,
            )

        def errors_for(*events: dict[str, Any]) -> list[str]:
            receipt = StreamReceipt()
            for event in events:
                receipt.process(event)
            return receipt.protocol_errors()

        self.assertEqual(
            errors_for(
                terminal("failed", "timeout"),
                error("timeout"),
                stream_event("done"),
            ),
            [],
        )
        self.assertEqual(
            errors_for(
                terminal("interrupted", "canceled"),
                error("canceled"),
                stream_event("done"),
            ),
            [],
        )

        completed_error = errors_for(
            terminal("completed", "resolved"),
            error("resolved"),
            stream_event("done"),
        )
        self.assertIn("completed_terminal_with_error", completed_error)

        mismatched_reason = errors_for(
            terminal("failed", "timeout"),
            error("model_error"),
            stream_event("done"),
        )
        self.assertIn("error_termination_reason_mismatch", mismatched_reason)

        untyped_failure = errors_for(
            terminal("failed", "resolved"),
            error("resolved"),
            stream_event("done"),
        )
        self.assertIn(
            "failure_terminal_status_reason_mismatch", untyped_failure
        )

        missing_error = errors_for(
            terminal("failed", "timeout"),
            stream_event("done"),
        )
        self.assertIn("failure_error_count", missing_error)

        duplicate_error = errors_for(
            terminal("failed", "timeout"),
            error("timeout"),
            error("timeout"),
            stream_event("done"),
        )
        self.assertIn("failure_error_count", duplicate_error)

        old_order = errors_for(
            error("timeout"),
            terminal("failed", "timeout"),
            stream_event("done"),
        )
        self.assertIn("error_before_terminal_metadata", old_order)

        late_content = errors_for(
            terminal("failed", "timeout"),
            stream_event("content", content="late"),
            error("timeout"),
            stream_event("done"),
        )
        self.assertIn("event_after_terminal_metadata", late_content)

        late_tool = errors_for(
            terminal("failed", "timeout"),
            stream_event("tool_use", id="late", name="read_file", input={}),
            stream_event(
                "tool_result",
                id="late",
                name="read_file",
                status="success",
                output="redacted",
            ),
            error("timeout"),
            stream_event("done"),
        )
        self.assertIn("event_after_terminal_metadata", late_tool)

    def test_multi_rejects_root_read_before_handoff_and_failed_child(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            workspace = Path(temporary)
            (workspace / "ranges.py").write_text("before", encoding="utf-8")
            initial = snapshot_workspace(workspace)
            receipt = StreamReceipt()
            for item in (
                stream_event("tool_use", id="r", name="read_file", input={}),
                stream_event(
                    "tool_result", id="r", name="read_file", status="success", output="ok"
                ),
                stream_event(
                    "tool_use",
                    id="a",
                    name="agent",
                    input={
                        "prompt": "只读检查范围合并缺陷",
                        "type": "explore",
                        "fork_context": False,
                        "expected_artifact": "缺陷诊断",
                        "max_steps": 4,
                        "wall_time_secs": 120,
                        "allowed_tools": ["read_file", "list_dir", "grep_files"],
                    },
                ),
                stream_event(
                    "child_started",
                    call_id="a",
                    child_run_id="child-1",
                    depth=1,
                    started_at="2026-07-18T00:00:00Z",
                ),
                stream_event(
                    "tool_result",
                    id="a",
                    name="agent",
                    status="success",
                    output=json.dumps({"agent_id": "child-1"}),
                ),
                stream_event(
                    "child_finished",
                    call_id="a",
                    child_run_id="child-1",
                    status="failed",
                    result_present=False,
                    completed_at="2026-07-18T00:00:01Z",
                ),
            ):
                receipt.process(item, workspace)
            contract = lane_contract("multi", receipt, initial)
            self.assertFalse(contract["passed"])
            self.assertIn("root_tool_before_handoff", contract["reasons"])
            self.assertIn("child_must_complete", contract["reasons"])

    def test_official_reprice_and_terminal_projection_are_fail_closed(self) -> None:
        runtime_sha = sha256_bytes(b"runtime")
        hit, miss, output = 100, 50, 20
        cost = official_cost_usd(DEFAULT_MODEL, hit, miss, output)
        self.assertIsNotNone(cost)
        raw = {
            "provider": "deepseek",
            "model": DEFAULT_MODEL,
            "status": "completed",
            "termination_reason": "resolved",
            "route_source": "explicit_or_configured",
            "approval_posture": "auto_tools",
            "sandbox_posture": "workspace-write",
            "binary_sha256": runtime_sha,
            "prompt_sha256": sha256_bytes(b"prompt"),
            "tool_catalog_sha256": sha256_bytes(b"tools"),
            "api_request_count": 1,
            "api_request_completed": 1,
            "api_request_in_flight": 0,
            "api_request_limit": MAX_API_REQUESTS_PER_LANE,
            "api_request_budget_exhausted": False,
            "api_request_rejected_exhausted": 0,
            "api_request_rejected_after_seal_observed": 0,
            "transport_retry_count": 0,
            "usage_complete": True,
            "cost_complete": True,
            "usage_missing_responses": 0,
            "usage_incomplete_responses": 0,
            "billing_unknown_attempts": 0,
            "unpriced_usage_responses": 0,
            "usage_records_after_seal_observed": 0,
            "usage_response_count": 1,
            "standard_chat_response_count": 1,
            "strict_chat_response_count": 0,
            "fim_response_count": 0,
            "input_tokens": hit + miss,
            "output_tokens": output,
            "total_tokens": hit + miss + output,
            "prompt_cache_hit_tokens": hit,
            "prompt_cache_miss_tokens": miss,
            "cost_usd": cost,
            "cost_cny": 0.0,
            "surface_model_usage_buckets": [
                {
                    "model": DEFAULT_MODEL,
                    "api_surface": "standard_chat",
                    "response_count": 1,
                    "usage_response_count": 1,
                    "input_tokens": hit + miss,
                    "output_tokens": output,
                    "prompt_cache_hit_tokens": hit,
                    "prompt_cache_miss_tokens": miss,
                    "total_tokens": hit + miss + output,
                    "cost_usd": cost,
                    "cost_cny": 0.0,
                }
            ],
        }
        terminal = sanitize_terminal(raw)
        self.assertTrue(terminal_measurement_valid(terminal))
        self.assertTrue(
            terminal_execution_contract_valid(terminal, DEFAULT_MODEL, runtime_sha)
        )
        wrong_runtime = json.loads(json.dumps(raw))
        wrong_runtime["binary_sha256"] = sha256_bytes(b"wrong-runtime")
        wrong_runtime_terminal = sanitize_terminal(wrong_runtime)
        self.assertTrue(terminal_measurement_valid(wrong_runtime_terminal))
        self.assertFalse(
            terminal_execution_contract_valid(
                wrong_runtime_terminal, DEFAULT_MODEL, runtime_sha
            )
        )
        drift = json.loads(json.dumps(raw))
        drift["cost_usd"] = float(cost) + 0.001
        self.assertFalse(terminal_measurement_valid(sanitize_terminal(drift)))
        after_seal = json.loads(json.dumps(raw))
        after_seal["usage_records_after_seal_observed"] = 1
        self.assertFalse(terminal_measurement_valid(sanitize_terminal(after_seal)))
        unresolved = json.loads(json.dumps(raw))
        unresolved["termination_reason"] = "unresolved"
        self.assertFalse(terminal_measurement_valid(sanitize_terminal(unresolved)))

        failed = json.loads(json.dumps(raw))
        failed["status"] = "failed"
        failed["termination_reason"] = "timeout"
        self.assertTrue(terminal_measurement_valid(sanitize_terminal(failed)))
        canceled = json.loads(json.dumps(raw))
        canceled["status"] = "interrupted"
        canceled["termination_reason"] = "canceled"
        self.assertTrue(terminal_measurement_valid(sanitize_terminal(canceled)))
        self.assertEqual(classify_claim_outcome(False, False), (False, False))
        self.assertEqual(classify_claim_outcome(True, False), (False, True))

        exhausted = json.loads(json.dumps(failed))
        exhausted["termination_reason"] = "budget_exhausted"
        exhausted["api_request_budget_exhausted"] = True
        exhausted["api_request_rejected_exhausted"] = 1
        self.assertTrue(terminal_measurement_valid(sanitize_terminal(exhausted)))

        for reason, status in TERMINAL_REASON_STATUS.items():
            typed = {"termination_reason": reason, "status": status}
            self.assertTrue(terminal_status_reason_valid(typed))

    def test_false_success_requires_a_rejected_completed_claim(self) -> None:
        verifier_passed = False
        lane_passed = True
        claim_acceptance_passed = verifier_passed and lane_passed
        self.assertEqual(
            classify_claim_outcome(True, claim_acceptance_passed),
            (False, True),
        )
        self.assertEqual(
            classify_claim_outcome(False, claim_acceptance_passed),
            (False, False),
        )

    def test_atomic_output_publishes_complete_plan_without_incomplete_file(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            destination = Path(temporary) / "plan.jsonl"
            args = argparse.Namespace(
                self_test=False,
                dry_run=True,
                acknowledge_cost=False,
                key_file=None,
                baseline_binary=None,
                baseline_revision=None,
                candidate_binary=None,
                candidate_revision=None,
                output=str(destination),
                runs_per_cell=MIN_RUNS_PER_CELL,
                lane="all",
                model=DEFAULT_MODEL,
            )
            self.assertEqual(run_with_atomic_output(args), 0)
            records = [
                json.loads(line)
                for line in destination.read_text(encoding="utf-8").splitlines()
            ]
            self.assertEqual([record["record_type"] for record in records], ["plan"])
            self.assertEqual(
                [path for path in Path(temporary).iterdir() if ".incomplete." in path.name],
                [],
            )


def run_self_tests() -> int:
    suite = unittest.defaultTestLoader.loadTestsFromTestCase(HarnessSelfTests)
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    return 0 if result.wasSuccessful() else 1


def bounded_runs_per_cell(value: str) -> int:
    try:
        parsed = int(value)
    except ValueError as error:
        raise argparse.ArgumentTypeError("runs per cell must be an integer") from error
    if not 1 <= parsed <= MAX_RUNS_PER_CELL:
        raise argparse.ArgumentTypeError(
            f"runs per cell must be between 1 and {MAX_RUNS_PER_CELL}"
        )
    return parsed


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--dry-run", action="store_true")
    mode.add_argument("--self-test", action="store_true")
    parser.add_argument("--acknowledge-cost", action="store_true")
    parser.add_argument("--key-file")
    parser.add_argument("--baseline-binary")
    parser.add_argument("--baseline-revision")
    parser.add_argument("--candidate-binary")
    parser.add_argument("--candidate-revision")
    parser.add_argument(
        "--output",
        help="atomically publish complete JSONL to this path instead of stdout",
    )
    parser.add_argument(
        "--runs-per-cell",
        type=bounded_runs_per_cell,
        default=DEFAULT_RUNS_PER_CELL,
    )
    parser.add_argument("--lane", choices=("all", "single", "multi"), default="all")
    parser.add_argument("--model", choices=SUPPORTED_MODELS, default=DEFAULT_MODEL)
    return parser.parse_args()


def run_with_atomic_output(args: argparse.Namespace) -> int:
    global ACTIVE_OUTPUT_STREAM
    if not args.output:
        return run_suite(args)
    destination = Path(args.output).expanduser()
    if not destination.is_absolute():
        destination = Path.cwd() / destination
    parent = destination.parent
    if not parent.is_dir():
        return fail_preflight("output_parent_unavailable")
    temporary: Path | None = None
    stream: TextIO | None = None
    published = False
    try:
        handle = tempfile.NamedTemporaryFile(
            mode="w",
            encoding="utf-8",
            dir=parent,
            prefix=f".{destination.name}.incomplete.",
            delete=False,
        )
        stream = handle
        temporary = Path(handle.name)
        os.chmod(temporary, 0o600)
        ACTIVE_OUTPUT_STREAM = stream
        status = run_suite(args)
        stream.flush()
        os.fsync(stream.fileno())
        if stream.tell() == 0:
            raise RuntimeError("empty_atomic_output")
        stream.close()
        stream = None
        ACTIVE_OUTPUT_STREAM = None
        os.replace(temporary, destination)
        published = True
        try:
            directory_fd = os.open(parent, os.O_RDONLY)
            try:
                os.fsync(directory_fd)
            finally:
                os.close(directory_fd)
        except OSError:
            # Some platforms/filesystems do not support directory fsync. The
            # file itself was fsynced and replace remains atomic.
            pass
        return status
    finally:
        ACTIVE_OUTPUT_STREAM = None
        if stream is not None:
            stream.close()
        if temporary is not None and not published:
            temporary.unlink(missing_ok=True)


if __name__ == "__main__":
    try:
        raise SystemExit(run_with_atomic_output(parse_args()))
    except KeyboardInterrupt:
        emit(
            {
                "record_type": "error",
                "error_code": "interrupted",
            },
            sys.stderr,
        )
        raise SystemExit(130) from None
    except Exception:
        # Never serialize exception text: a dependency could include response,
        # tool, environment, or path details. The typed code is enough for the
        # operator to rerun the offline self-test before investigating locally.
        emit(
            {
                "record_type": "error",
                "error_code": "harness_internal_error",
            },
            sys.stderr,
        )
        raise SystemExit(2) from None
