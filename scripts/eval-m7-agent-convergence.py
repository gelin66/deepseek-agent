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
MANIFEST_PATH = ROOT / "eval/manifests/m7-a-agent-convergence-ab-v1.json"
CANARY_PATH = ROOT / "scripts/eval-m6-writer-canary.py"
RESULT_SCHEMA = "codewhale.eval.m7-agent-convergence.v1"
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
    return result.stdout.strip()


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
            "max_api_requests": RESOURCES["max_physical_api_requests_per_arm"],
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
) -> list[dict[str, Any]]:
    result = client.call(CANARY.query("events", run_id, request_id))
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
    try:
        requests = {
            key: int(root[key]) + int(child[key])
            for key in ("started", "completed", "in_flight")
        }
        result = {
            "requests": requests,
            "root": {key: int(root[key]) for key in ("started", "completed", "in_flight", "retries")},
            "child": {key: int(child[key]) for key in ("started", "completed", "in_flight", "retries")},
            "runtime_retries": int(run.get("runtime_retries", 0)),
            "transport_retries": int(accounting.get("transport_retries", 0)),
            "billing_unknown_attempts": int(accounting.get("billing_unknown_attempts", 0)),
            "usage": {field: int(usage.get(field, 0)) for field in USAGE_FIELDS},
            "cost_nanousd": int(accounting.get("cost_nanousd", 0)),
            "cost_nanocny": int(accounting.get("cost_nanocny", 0)),
            "complete": accounting.get("complete"),
            "usage_complete": accounting.get("usage_complete"),
            "billing_unknown": accounting.get("billing_unknown"),
            "unpriced": accounting.get("unpriced"),
            "sealed": accounting.get("sealed"),
        }
    except (KeyError, TypeError, ValueError) as error:
        raise EvaluationError("accounting_shape_invalid") from error
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
    )
    return result


def latest_revision(workspace_state: Any) -> str | None:
    if not isinstance(workspace_state, dict):
        return None
    revision = workspace_state.get("revision")
    if isinstance(revision, dict) and revision.get("status") == "known":
        value = revision.get("sha256")
        return value if isinstance(value, str) else None
    return None


def verification_summary(task_id: str, events: list[dict[str, Any]], run: dict[str, Any]) -> dict[str, Any]:
    created = run_created(events)
    contract = created.get("task_contract", {})
    acceptance = contract.get("definition", {}).get("acceptance", [])
    frozen = acceptance[0].get("verifier", {}) if len(acceptance) == 1 else {}
    commits = event_values(events, "host_verification_committed")
    receipts = [event.get("receipt") for event in commits if isinstance(event.get("receipt"), dict)]
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
    terminal = terminal_events[0].get("outcome", {}).get("terminal", {}) if len(terminal_events) == 1 else {}
    decision = terminal.get("decision", {}) if isinstance(terminal, dict) else {}
    receipt = receipts[0] if len(receipts) == 1 else {}
    receipt_revision = latest_revision(receipt.get("workspace_state")) if isinstance(receipt, dict) else None
    frozen_hash = canonical_hash(frozen) if frozen else None
    resolved_hash = canonical_hash(verifier_spec(task_id, resolved=True))
    valid = (
        terminal_state(run) == "completed"
        and len(commits) == 1
        and len(receipts) == 1
        and frozen == verifier_spec(task_id, resolved=True)
        and observed_specs == [resolved_hash]
        and verdicts == ["passed"]
        and receipt.get("verifier") == frozen
        and receipt.get("acceptance_id") == MANIFEST["tasks"][task_id]["acceptance_id"]
        and receipt_revision is not None
        and receipt_revision == revisions[0]
        and decision.get("workspace_state") == receipt.get("workspace_state")
        and repeated_same_revision == 0
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
    return {
        "valid": valid,
        "contract_spec_sha256": frozen_hash,
        "caller_spec_sha256": canonical_hash(verifier_spec(task_id, resolved=False)),
        "resolved_spec_sha256": resolved_hash,
        "observed_spec_sha256": observed_specs,
        "host_commit_count": len(commits),
        "receipt_count": len(receipts),
        "verdicts": verdicts,
        "repeated_same_revision": repeated_same_revision,
        "completion_proposals": len(event_values(events, "completion_proposed")),
        "completion_rejections": len(event_values(events, "completion_rejected")),
        "rejections": rejection_facts,
    }


def tool_summary(events: list[dict[str, Any]]) -> dict[str, Any]:
    names: list[str] = []
    argument_hashes: list[str] = []
    outcomes: Counter[str] = Counter()
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
        outcomes[f"{name}:{outcome.get('invocation')}:{outcome.get('operation')}:{outcome.get('retry')}"] += 1
        observation = outcome.get("verifier_observation")
        if isinstance(observation, dict) and isinstance(observation.get("verdict"), str):
            verifier_verdicts.append(observation["verdict"])
    return {
        "names": names,
        "argument_sha256": argument_hashes,
        "outcomes": dict(sorted(outcomes.items())),
        "verifier_verdicts": verifier_verdicts,
    }


def t3_temporal_valid(events: list[dict[str, Any]]) -> bool:
    failure_sequence = None
    write_sequence = None
    host_pass_sequence = None
    for stored in events:
        event = stored.get("event", {})
        if event_kind(stored) == "tool_outcome_committed":
            observation = event.get("outcome", {}).get("verifier_observation", {})
            if event.get("name") == "run_verifiers" and observation.get("verdict") == "failed" and failure_sequence is None:
                failure_sequence = stored.get("sequence")
            if event.get("name") in {"apply_patch", "edit_file"} and event.get("outcome", {}).get("side_effect") == "applied":
                write_sequence = stored.get("sequence")
        if event_kind(stored) == "host_verification_committed" and isinstance(event.get("receipt"), dict):
            host_pass_sequence = stored.get("sequence")
            if event["receipt"].get("lineage", {}).get("policy") != "failed_write_pass":
                return False
    return (
        isinstance(failure_sequence, int)
        and isinstance(write_sequence, int)
        and isinstance(host_pass_sequence, int)
        and failure_sequence < write_sequence < host_pass_sequence
    )


def child_summary(
    client: Any,
    root_events: list[dict[str, Any]],
    suffix: str,
    expected_event_schema: int,
) -> dict[str, Any]:
    prepared = event_values(root_events, "agent_task_prepared")
    children = []
    for index, event in enumerate(prepared):
        task = event.get("task", {})
        child_id = task.get("child_run_id")
        require(isinstance(child_id, str), "child_id_missing")
        result = client.call(CANARY.query("get", child_id, f"m7-child-get-{suffix}-{index}"))
        require(result.get("kind") == "run", "child_run_missing")
        child = result["run"]
        child_events = events(
            client,
            child_id,
            f"m7-child-events-{suffix}-{index}",
            expected_event_schema,
        )
        children.append(
            {
                "terminal": terminal_summary(child),
                "tool": tool_summary(child_events),
                "workspace_access": task.get("workspace", {}).get("access"),
                "child_finished": len(
                    [
                        value
                        for value in event_values(root_events, "child_finished")
                        if value.get("child_run_id") == child_id
                    ]
                ),
            }
        )
    return {"count": len(children), "children": children}


def child_expectation_valid(task_id: str, summary: dict[str, Any]) -> bool:
    expectation = MANIFEST["tasks"][task_id]["child_expectation"]
    if expectation in {"forbidden", "zero"}:
        return summary["count"] == 0
    if expectation == "exactly_one_read_only":
        if summary["count"] != 1:
            return False
        child = summary["children"][0]
        return (
            child["workspace_access"] == "read_only"
            and child["terminal"]["state"] == "completed"
            and child["child_finished"] == 1
            and all(name in MANIFEST["tool_policy"]["readonly_child_tools"] for name in child["tool"]["names"])
            and not any(name in {"apply_patch", "edit_file", "exec_shell", "run_tests", "run_verifiers"} for name in child["tool"]["names"])
        )
    return False


def changed_files(workspace: Path) -> list[str]:
    lines = run_git(workspace, "status", "--porcelain=v1", "--untracked-files=all").splitlines()
    paths = []
    for line in lines:
        if not line:
            continue
        raw = line[3:]
        paths.append(raw.split(" -> ")[-1])
    return sorted(paths)


def external_verifier(workspace: Path) -> dict[str, Any]:
    before = snapshot_tree(workspace)
    started = time.monotonic()
    environment = safe_env()
    environment["PYTHONDONTWRITEBYTECODE"] = "1"
    result = subprocess.run(
        ["/usr/bin/python3", "-I", "-B", "_eval_verifier.py", "."],
        cwd=workspace,
        env=environment,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=30,
        check=False,
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


def state_schema(codewhale_home: Path, expected: int) -> dict[str, Any]:
    database = codewhale_home / "state.db"
    connection = sqlite3.connect(f"file:{database}?mode=ro", uri=True)
    try:
        row = connection.execute("PRAGMA user_version").fetchone()
    finally:
        connection.close()
    version = row[0] if row else None
    return {"valid": version == expected, "version": version, "sha256": file_hash(database)}


def wait_for_terminal(client: Any, process: Any, root_id: str, started: float, suffix: str) -> dict[str, Any]:
    deadline = started + RESOURCES["harness_wall_time_seconds"]
    poll = 0
    while True:
        result = client.call(CANARY.query("get", root_id, f"m7-get-{suffix}-{poll}"))
        require(result.get("kind") == "run", "run_view_missing")
        run = result["run"]
        if run.get("terminal") is not None:
            return run
        require(process.poll() is None, "app_server_exited")
        require(time.monotonic() < deadline, "run_deadline_exceeded")
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
                [str(binary), "--provider", "deepseek", "app-server", "--stdio"],
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
            response = client.call(start_command(task_id, workspace, f"m7-start-{suffix}"))
            require(response.get("kind") == "run", "start_run_missing")
            root_id = response["run"].get("run_id")
            require(isinstance(root_id, str), "root_run_id_missing")
            run = wait_for_terminal(client, process, root_id, started, suffix)
            root_events = events(
                client,
                root_id,
                f"m7-events-{suffix}",
                schemas["runtime_event"],
            )
            children = child_summary(
                client,
                root_events,
                suffix,
                schemas["runtime_event"],
            )
            accounting = usage_summary(run)
            verification = verification_summary(task_id, root_events, run)
            tools = tool_summary(root_events)
            external = external_verifier(workspace)
            changed = changed_files(workspace)
            expected_changed = sorted(MANIFEST["tasks"][task_id]["expected_changed_files"])
            scope_valid = changed == expected_changed
            child_valid = child_expectation_valid(task_id, children)
            temporal_valid = task_id != "t3" or t3_temporal_valid(root_events)
            verified = (
                verification["valid"]
                and external["passed"]
                and external["workspace_unchanged"]
                and scope_valid
                and child_valid
                and temporal_valid
                and accounting["valid"]
            )
            false_success = terminal_state(run) == "completed" and not verified
            first_request = event_values(root_events, "model_request_prepared")
            first_request_hash = canonical_hash(first_request[0].get("request")) if first_request else None
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
                "task_definition_sha256": canonical_hash(task_definition(task_id)),
                "model": MODEL,
                "api_surface": "standard_chat",
                "terminal": terminal_summary(run),
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
                "scope_valid": scope_valid,
                "first_model_request_sha256": first_request_hash,
                "event_counts": dict(sorted(Counter(event_kind(event) for event in root_events).items())),
                "protocol_schemas": schemas,
                "state_schema": state_schema(codewhale_home, schemas["state"]),
                "wall_time_ms": int((time.monotonic() - started) * 1000),
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


def summarize(arms: list[dict[str, Any]]) -> dict[str, Any]:
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
            "requests": sum(arm["accounting"]["requests"]["started"] for arm in selected),
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
    hard_gates = {
        "false_success_zero": by_variant["candidate"]["false_success"] == 0,
        "per_task_success_non_regression": all(
            task_cells[task]["candidate"]["verified_success"] >= task_cells[task]["baseline"]["verified_success"]
            for task in TASK_IDS
        ),
        "formal_cells_complete": all(
            task_cells[task][variant]["arms"] == MANIFEST["experiment"]["runs_per_variant_task"]
            for task in TASK_IDS
            for variant in VARIANTS
        ),
        "candidate_accounting_valid": all(arm["accounting"]["valid"] for arm in arms if arm["variant"] == "candidate"),
        "candidate_spec_exact": all(
            arm["verification"]["contract_spec_sha256"] == arm["verification"]["resolved_spec_sha256"]
            for arm in arms
            if arm["variant"] == "candidate"
        ),
        "candidate_same_revision_repeat_zero": by_variant["candidate"]["same_revision_repeats"] == 0,
    }
    product_metric_eligible = all(hard_gates.values()) and all(arm["accounting"]["valid"] for arm in arms)
    if not product_metric_eligible:
        decision = "hold"
    elif delta >= 3:
        decision = "keep"
    else:
        decision = "reject"
    return {
        "by_variant": by_variant,
        "task_cells": task_cells,
        "verified_success_delta": delta,
        "hard_gates": hard_gates,
        "product_metric_eligible": product_metric_eligible,
        "decision": decision,
    }


def preflight_binary(path: Path, revision: str) -> dict[str, Any]:
    require(path.is_file() and os.access(path, os.X_OK), "binary_unavailable")
    require(len(revision) == 40 and all(character in "0123456789abcdef" for character in revision), "revision_invalid")
    with tempfile.TemporaryDirectory(prefix="codewhale-m7-probe-") as raw:
        workspace = Path(raw)
        return {
            "revision": revision,
            "sha256": file_hash(path),
            "identity": CANARY.probe_binary(path, workspace, safe_env(), revision),
        }


def write_private_json(path: Path, value: Any, secret: bytes) -> None:
    encoded = canonical_bytes(value) + b"\n"
    require(secret not in encoded, "key_in_result")
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".partial")
    descriptor = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    try:
        os.write(descriptor, encoded)
        os.fsync(descriptor)
    finally:
        os.close(descriptor)
    os.chmod(temporary, 0o600)
    os.replace(temporary, path)
    require(stat.S_IMODE(path.stat().st_mode) == 0o600, "result_mode_invalid")


def run_suite(args: argparse.Namespace, *, formal: bool) -> dict[str, Any]:
    require(args.acknowledge_cost, "cost_acknowledgement_required")
    require(args.key_file is not None, "key_file_required")
    validate_frozen_manifest()
    key = CANARY.read_key(args.key_file.expanduser())
    secret = key.encode()
    binaries = {
        "baseline": args.baseline_binary.expanduser().resolve(),
    }
    revisions = {"baseline": args.baseline_revision}
    if formal:
        require(args.candidate_binary is not None and args.candidate_revision is not None, "candidate_identity_required")
        binaries["candidate"] = args.candidate_binary.expanduser().resolve()
        revisions["candidate"] = args.candidate_revision
    identities = {variant: preflight_binary(binary, revisions[variant]) for variant, binary in binaries.items()}
    schedule = formal_schedule() if formal else [
        {"task_id": task_id, "run_index": 0, "variant": "baseline", "arm_position": 1}
        for task_id in TASK_IDS
    ]
    maximum_cost = RESOURCES["formal_suite_known_cost_usd" if formal else "diagnostic_known_cost_usd"]
    record = {
        "schema": RESULT_SCHEMA,
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
        "abort": None,
    }
    for scheduled in schedule:
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
            known_cost = sum(value["accounting"]["cost_nanousd"] for value in record["arms"]) / 1_000_000_000
            if not arm["accounting"]["valid"]:
                record["abort"] = {"code": "measurement_invalid", "task_id": arm["task_id"], "variant": arm["variant"]}
                break
            if known_cost > maximum_cost:
                record["abort"] = {"code": "known_cost_limit", "known_cost_usd": known_cost}
                break
        except (EvaluationError, CANARY.Failure) as error:
            code = error.code
            record["abort"] = {"code": code, "details_sha256": canonical_hash(getattr(error, "details", {}))}
            if code in SECRET_FAILURES:
                record["arms"] = []
            break
        write_private_json(args.output, record, secret)
    record["aggregate"] = summarize(record["arms"]) if formal and record["arms"] else None
    record["product_metric_eligible"] = bool(formal and record["abort"] is None and record["aggregate"]["product_metric_eligible"])
    write_private_json(args.output, record, secret)
    key = ""
    return record


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

    def test_summary_requires_three_more_verified_arms(self) -> None:
        arms = []
        for task_id in TASK_IDS:
            for variant in VARIANTS:
                for run_index in range(1, 5):
                    verified = variant == "candidate" and task_id == "t1" and run_index <= 3
                    arms.append(
                        {
                            "task_id": task_id,
                            "variant": variant,
                            "verified_success": verified,
                            "false_success": False,
                            "terminal": {"state": "completed" if verified else "blocked"},
                            "verification": {
                                "completion_rejections": 0,
                                "repeated_same_revision": 0,
                                "contract_spec_sha256": "same",
                                "resolved_spec_sha256": "same",
                            },
                            "accounting": {
                                "valid": True,
                                "requests": {"started": 1},
                                "usage": {field: 1 for field in USAGE_FIELDS},
                                "cost_nanousd": 1,
                                "cost_nanocny": 1,
                            },
                            "wall_time_ms": 1,
                        }
                    )
        result = summarize(arms)
        self.assertEqual(result["verified_success_delta"], 3)
        self.assertEqual(result["decision"], "keep")


def freeze_report() -> dict[str, Any]:
    schedule = formal_schedule()
    return {
        "manifest_content_sha256": manifest_content_hash(),
        "harness_sha256": file_hash(Path(__file__)),
        "canary_helper_sha256": file_hash(CANARY_PATH),
        "schedule_sha256": canonical_hash(schedule),
        "fixture_tree_sha256": {task: fixture_hash(task) for task in TASK_IDS},
        "task_definition_sha256": {task: canonical_hash(task_definition(task)) for task in TASK_IDS},
        "caller_verifier_spec_sha256": {task: canonical_hash(verifier_spec(task, resolved=False)) for task in TASK_IDS},
        "resolved_verifier_spec_sha256": {task: canonical_hash(verifier_spec(task, resolved=True)) for task in TASK_IDS},
        "formal_arms": len(schedule),
    }


def validate_frozen_manifest() -> None:
    require(MANIFEST.get("status") == "frozen_before_baseline_api", "manifest_not_frozen")
    expected = MANIFEST.get("frozen_hashes")
    require(isinstance(expected, dict), "frozen_hashes_missing")
    actual = freeze_report()
    comparisons = {
        "manifest_content_sha256_excluding_frozen_hashes": actual["manifest_content_sha256"],
        "harness_sha256": actual["harness_sha256"],
        "canary_helper_sha256": actual["canary_helper_sha256"],
        "schedule_sha256": actual["schedule_sha256"],
        "fixture_tree_sha256": actual["fixture_tree_sha256"],
        "task_definition_sha256": actual["task_definition_sha256"],
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
    except EvaluationError as error:
        print(f"evaluation_error:{error.code}", file=sys.stderr)
        raise SystemExit(2)
