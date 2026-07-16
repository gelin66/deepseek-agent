#!/usr/bin/env python3
"""M4-A credentialed crash/reopen/resume canary.

The supervisor persists only redacted measurements. Model text, reasoning,
tool arguments/results, stderr, event JSON, and credentials are never emitted.
"""

from __future__ import annotations

import argparse
import errno
import hashlib
import importlib.util
import json
import os
import shutil
import signal
import sqlite3
import stat
import subprocess
import sys
import tempfile
import threading
import time
import uuid
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
HARNESS_PATH = ROOT / "scripts" / "eval-deepseek-exec.py"
SCHEMA = "codewhale.eval.m4a-resume-canary.v2"
SAFE_PRE_IO_KINDS = {"run_created", "model_request_prepared"}
USAGE_DELTA_FIELDS = (
    "input_tokens",
    "output_tokens",
    "total_tokens",
    "prompt_cache_hit_tokens",
    "prompt_cache_miss_tokens",
    "prompt_cache_write_tokens",
    "reasoning_tokens",
    "reasoning_replay_tokens",
    "usage_response_count",
    "cost_usd",
    "cost_cny",
)


def load_harness() -> Any:
    sys.dont_write_bytecode = True
    spec = importlib.util.spec_from_file_location("m4a_eval_harness", HARNESS_PATH)
    if spec is None or spec.loader is None:
        raise RuntimeError("harness_import_failed")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def length_prefixed(digest: Any, value: Any) -> None:
    raw = str(value).encode("utf-8")
    digest.update(len(raw).to_bytes(8, "big"))
    digest.update(raw)


def event_digest(rows: list[tuple[Any, ...]]) -> str:
    digest = hashlib.sha256()
    for row in rows:
        for value in row:
            length_prefixed(digest, value)
    return "sha256:" + digest.hexdigest()


def read_event_prefix_digest(db_path: Path, run_id: str, count: int) -> str | None:
    if count < 0 or not db_path.is_file():
        return None
    connection = sqlite3.connect(
        f"file:{db_path}?mode=ro", uri=True, timeout=0.02, isolation_level=None
    )
    try:
        connection.execute("PRAGMA query_only = ON")
        rows = connection.execute(
            """
            SELECT sequence, event_id, schema_version, occurred_at_unix_ms,
                   terminal, event_json
            FROM agent_run_events
            WHERE run_id = ? ORDER BY sequence LIMIT ?
            """,
            (run_id, count),
        ).fetchall()
    finally:
        connection.close()
    if len(rows) != count:
        return None
    return event_digest(rows)


def read_db_state(db_path: Path) -> dict[str, Any] | None:
    if not db_path.is_file():
        return None
    connection = sqlite3.connect(
        f"file:{db_path}?mode=ro", uri=True, timeout=0.02, isolation_level=None
    )
    try:
        connection.execute("PRAGMA query_only = ON")
        run_rows = connection.execute(
            """
            SELECT run_id, last_sequence, terminal, execution_epoch,
                   lease_owner_id, lease_owner_pid,
                   pending_model_attempt_id, pending_model_in_flight
            FROM agent_runs ORDER BY created_at_unix_ms, run_id
            """
        ).fetchall()
        if len(run_rows) != 1:
            return None
        run = run_rows[0]
        rows = connection.execute(
            """
            SELECT sequence, event_id, schema_version, occurred_at_unix_ms,
                   terminal, event_json
            FROM agent_run_events WHERE run_id = ? ORDER BY sequence
            """,
            (run[0],),
        ).fetchall()
    finally:
        connection.close()
    kinds: list[str] = []
    event_run_ids: list[str | None] = []
    terminal_outcome_run_ids: list[str | None] = []
    applied_side_effect_rows: list[tuple[Any, ...]] = []
    for row in rows:
        event = json.loads(row[5])
        event_body = event.get("event", {})
        kind = event_body.get("kind")
        kinds.append(kind if isinstance(kind, str) else "invalid")
        event_run_ids.append(event.get("run_id"))
        if kind == "terminal":
            terminal_outcome_run_ids.append(
                event_body.get("outcome", {}).get("run_id")
            )
        if kind == "tool_outcome_committed":
            outcome = event_body.get("outcome", {})
            if outcome.get("side_effect") == "applied":
                applied_side_effect_rows.append(
                    (
                        row[0],
                        event_body.get("call_id"),
                        event_body.get("name"),
                        outcome.get("side_effect"),
                        outcome.get("workspace_revision"),
                    )
                )
    sequences = [int(row[0]) for row in rows]
    event_ids = [str(row[1]) for row in rows]
    terminal_count = sum(int(row[4]) for row in rows)
    return {
        "run_id": str(run[0]),
        "last_sequence": int(run[1]),
        "terminal": bool(run[2]),
        "execution_epoch": int(run[3]),
        "lease_owner_id": str(run[4]) if run[4] is not None else None,
        "lease_owner_id_present": run[4] is not None,
        "lease_owner_pid": int(run[5]) if run[5] is not None else None,
        "pending_model_attempt_id_present": run[6] is not None,
        "pending_model_in_flight": bool(run[7]),
        "event_count": len(rows),
        "event_digest": event_digest(rows),
        "event_kinds": kinds,
        "sequences_contiguous": sequences == list(range(1, len(rows) + 1)),
        "unique_event_id_count": len(set(event_ids)),
        "event_ids_unique": len(event_ids) == len(set(event_ids)),
        "event_run_ids_match": all(value == run[0] for value in event_run_ids),
        "terminal_outcome_run_ids_match": all(
            value == run[0] for value in terminal_outcome_run_ids
        ),
        "terminal_count": terminal_count,
        "terminal_last": terminal_count == 1 and bool(rows) and bool(rows[-1][4]),
        "model_request_count": kinds.count("model_request_in_flight"),
        "tool_side_effect_count": len(applied_side_effect_rows),
        "tool_side_effect_digest": event_digest(applied_side_effect_rows),
    }


def safe_pre_io(state: dict[str, Any], expected_pid: int) -> bool:
    return bool(
        not state["terminal"]
        and state["terminal_count"] == 0
        and not state["pending_model_in_flight"]
        and state["lease_owner_id_present"]
        and state["lease_owner_pid"] == expected_pid
        and state["event_count"] >= 1
        and state["sequences_contiguous"]
        and state["event_ids_unique"]
        and state["event_run_ids_match"]
        and set(state["event_kinds"]).issubset(SAFE_PRE_IO_KINDS)
        and "run_created" in state["event_kinds"]
    )


def wait_for_stop(pid: int, timeout: float) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        waited, status = os.waitpid(pid, os.WNOHANG | os.WUNTRACED)
        if waited == pid and os.WIFSTOPPED(status):
            return
        if waited == pid and (os.WIFEXITED(status) or os.WIFSIGNALED(status)):
            raise RuntimeError("capture_process_exited_before_stop")
        time.sleep(0.001)
    raise RuntimeError("capture_initial_stop_timeout")


def process_is_dead(pid: int) -> bool:
    try:
        os.kill(pid, 0)
    except OSError as error:
        return error.errno == errno.ESRCH
    return False


def exec_command(harness: Any, binary: Path, model: str, run_id: str | None) -> list[str]:
    command = harness.exec_command(binary, "single", model)
    if run_id is None:
        return command
    return command[:-1] + ["--resume", run_id]


def no_key_environment(harness: Any, state_root: Path) -> dict[str, str]:
    environment = harness.child_environment("placeholder-not-a-key", state_root)
    for name in list(environment):
        upper = name.upper()
        if "API_KEY" in upper or upper.endswith("_TOKEN") or upper in {
            "DEEPSEEK_API_KEY",
            "CODEWHALE_CLI_API_KEY",
        }:
            environment.pop(name, None)
    return environment


def checked_output(command: list[str], cwd: Path) -> str:
    completed = subprocess.run(
        command,
        cwd=cwd,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        text=True,
        timeout=30,
        check=False,
    )
    if completed.returncode != 0:
        raise RuntimeError(f"command_failed:{command[0]}:{completed.returncode}")
    return completed.stdout.strip()


def verified_source_identity(source_root: Path, revision: str) -> dict[str, Any]:
    source_root = source_root.resolve()
    head = checked_output(["git", "rev-parse", "HEAD"], source_root)
    expected = checked_output(
        ["git", "rev-parse", "--verify", f"{revision}^{{commit}}"], source_root
    )
    if head != expected:
        raise RuntimeError("candidate_revision_does_not_match_source_head")
    status = checked_output(
        ["git", "status", "--porcelain", "--untracked-files=all"], source_root
    )
    if status:
        raise RuntimeError("candidate_source_worktree_not_clean")
    return {
        "git_commit": head,
        "git_tree": checked_output(["git", "rev-parse", "HEAD^{tree}"], source_root),
        "source_root": str(source_root),
        "worktree_clean": True,
        "revision_matches_head": True,
    }


def build_release_candidate(source_root: Path, candidate: Path) -> dict[str, Any]:
    expected_candidate = source_root.resolve() / "target" / "release" / "codewhale"
    if candidate.resolve() != expected_candidate:
        raise RuntimeError("built_candidate_path_must_be_target_release_codewhale")
    command = [
        "cargo",
        "build",
        "--release",
        "--locked",
        "--offline",
        "-p",
        "codewhale-cli",
        "-p",
        "codewhale-tui",
    ]
    completed = subprocess.run(
        command,
        cwd=source_root,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        timeout=1800,
        check=False,
    )
    if completed.returncode != 0:
        raise RuntimeError(f"candidate_release_build_failed:{completed.returncode}")
    return {
        "built_by_supervisor": True,
        "command": command,
        "exit_code": completed.returncode,
        "cargo_version": checked_output(["cargo", "--version"], source_root),
        "rustc_version": checked_output(["rustc", "--version"], source_root),
    }


def run_exec_with_lease_observation(
    harness: Any,
    command: list[str],
    environment: dict[str, str],
    workspace: Path,
    db_path: Path,
    previous_epoch: int,
) -> tuple[Any, dict[str, Any] | None]:
    outcome: dict[str, Any] = {}

    def run() -> None:
        outcome["value"] = harness.run_exec_process(command, environment, workspace)

    thread = threading.Thread(target=run, name="m4a-resume-exec", daemon=True)
    thread.start()
    observed_owner: dict[str, Any] | None = None
    while thread.is_alive():
        try:
            state = read_db_state(db_path)
        except (sqlite3.Error, json.JSONDecodeError, OSError):
            state = None
        if (
            state is not None
            and state["execution_epoch"] > previous_epoch
            and state["lease_owner_id"] is not None
            and state["lease_owner_pid"] is not None
        ):
            observed_owner = {
                "owner_id": state["lease_owner_id"],
                "owner_pid": state["lease_owner_pid"],
                "execution_epoch": state["execution_epoch"],
            }
            break
        time.sleep(0.001)
    thread.join()
    if "value" not in outcome:
        raise RuntimeError("resume_process_outcome_missing")
    return outcome["value"], observed_owner


def terminal_safe(terminal: dict[str, Any] | None) -> dict[str, Any]:
    return dict(terminal or {})


def stable_terminal_receipt(terminal: dict[str, Any]) -> dict[str, Any]:
    """Drop projection-local latency while preserving terminal/accounting truth."""
    return {name: value for name, value in terminal.items() if name != "duration_ms"}


def terminal_usage_delta(terminal: dict[str, Any]) -> dict[str, int | float | None]:
    return {name: terminal.get(name) for name in USAGE_DELTA_FIELDS}


def capture_and_kill(
    harness: Any,
    command: list[str],
    environment: dict[str, str],
    workspace: Path,
    db_path: Path,
    capture_timeout: float,
) -> tuple[str, dict[str, Any], dict[str, Any]]:
    wrapper = [
        "/bin/sh",
        "-c",
        'kill -STOP $$; exec "$@"',
        "m4a-resume-canary",
        *command,
    ]
    process = subprocess.Popen(
        wrapper,
        cwd=workspace,
        env=environment,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        start_new_session=True,
    )
    environment.clear()
    wait_for_stop(process.pid, 5.0)
    os.killpg(process.pid, signal.SIGCONT)
    deadline = time.monotonic() + capture_timeout
    captured: dict[str, Any] | None = None
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError("capture_process_exited_before_safe_state")
        try:
            observed = read_db_state(db_path)
        except (sqlite3.Error, json.JSONDecodeError, OSError):
            observed = None
        if observed is not None and safe_pre_io(observed, process.pid):
            os.killpg(process.pid, signal.SIGSTOP)
            wait_for_stop(process.pid, 2.0)
            captured = read_db_state(db_path)
            if captured is not None and safe_pre_io(captured, process.pid):
                break
            os.killpg(process.pid, signal.SIGKILL)
            process.wait(timeout=5)
            raise RuntimeError("safe_state_advanced_before_stop")
        if observed is not None and (
            observed["pending_model_in_flight"]
            or "model_request_in_flight" in observed["event_kinds"]
            or "tool_execution_started" in observed["event_kinds"]
            or observed["terminal"]
        ):
            os.killpg(process.pid, signal.SIGSTOP)
            try:
                wait_for_stop(process.pid, 2.0)
            finally:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait(timeout=5)
            raise RuntimeError("capture_missed_safe_pre_io_window")
        time.sleep(0.00005)
    if captured is None:
        os.killpg(process.pid, signal.SIGKILL)
        process.wait(timeout=5)
        raise RuntimeError("capture_safe_state_timeout")
    os.killpg(process.pid, signal.SIGKILL)
    process.wait(timeout=5)
    if process.stdout is not None:
        process.stdout.close()
    after_kill = read_db_state(db_path)
    if after_kill is None:
        raise RuntimeError("state_missing_after_kill")
    crash = {
        "phase": "durable_pre_model_io",
        "kill_mechanism": "SIGKILL",
        "process_exit_code": process.returncode,
        "process_killed_by_sigkill": process.returncode == -signal.SIGKILL,
        "lease_owner_pid_dead": process_is_dead(process.pid),
        "lease_owner_id": captured["lease_owner_id"],
        "lease_owner_pid": captured["lease_owner_pid"],
        "run_id": captured["run_id"],
        "event_count": captured["event_count"],
        "last_sequence": captured["last_sequence"],
        "event_digest": captured["event_digest"],
        "event_kinds": captured["event_kinds"],
        "terminal_count": captured["terminal_count"],
        "pending_model_in_flight": captured["pending_model_in_flight"],
        "safe_pre_io_contract": safe_pre_io(captured, process.pid),
        "state_unchanged_by_kill": after_kill["event_digest"]
        == captured["event_digest"],
    }
    return captured["run_id"], captured, crash


def atomic_write(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.{uuid.uuid4().hex}.tmp")
    descriptor = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as stream:
            json.dump(value, stream, ensure_ascii=True, sort_keys=True)
            stream.write("\n")
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
        os.chmod(path, 0o600)
    finally:
        if temporary.exists():
            temporary.unlink()


def self_test() -> int:
    rows = [(1, "a", 3, 1, 0, "{}")]
    assert event_digest(rows) == event_digest(rows)
    assert event_digest(rows) != event_digest([(1, "b", 3, 1, 0, "{}")])
    safe = {
        "terminal": False,
        "terminal_count": 0,
        "pending_model_in_flight": False,
        "lease_owner_id_present": True,
        "lease_owner_pid": 7,
        "event_count": 1,
        "sequences_contiguous": True,
        "event_ids_unique": True,
        "event_run_ids_match": True,
        "event_kinds": ["run_created"],
    }
    assert safe_pre_io(safe, 7)
    unsafe = dict(safe, pending_model_in_flight=True)
    assert not safe_pre_io(unsafe, 7)
    terminal = {"status": "completed", "total_tokens": 3, "duration_ms": 12}
    replay = {"status": "completed", "total_tokens": 3, "duration_ms": 1}
    assert stable_terminal_receipt(terminal) == stable_terminal_receipt(replay)
    replay["total_tokens"] = 4
    assert stable_terminal_receipt(terminal) != stable_terminal_receipt(replay)
    usage = terminal_usage_delta({"input_tokens": 2, "total_tokens": 3})
    assert usage["input_tokens"] == 2
    assert usage["total_tokens"] == 3
    assert usage["output_tokens"] is None
    harness = load_harness()
    command = exec_command(harness, Path("/candidate"), "deepseek-v4-flash", None)
    assert all("key" not in part.lower() for part in command)
    print(json.dumps({"schema": SCHEMA, "self_test": "passed"}, sort_keys=True))
    return 0


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--candidate", type=Path)
    parser.add_argument("--candidate-revision")
    parser.add_argument("--source-root", type=Path, default=ROOT)
    parser.add_argument("--build-candidate", action="store_true")
    parser.add_argument("--key-file", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--model", default="deepseek-v4-flash")
    parser.add_argument("--capture-timeout", type=float, default=20.0)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        return self_test()
    if args.candidate is None:
        parser.error("--candidate is required")
    if not args.candidate_revision:
        parser.error("--candidate-revision is required")
    harness = load_harness()
    source_identity = verified_source_identity(
        args.source_root.resolve(), args.candidate_revision
    )
    build = {
        "built_by_supervisor": False,
        "command": None,
        "exit_code": None,
        "cargo_version": checked_output(
            ["cargo", "--version"], args.source_root.resolve()
        ),
        "rustc_version": checked_output(
            ["rustc", "--version"], args.source_root.resolve()
        ),
    }
    if args.build_candidate and not args.dry_run:
        build = build_release_candidate(
            args.source_root.resolve(), args.candidate.resolve()
        )
    identity = harness.binary_pair_identity(args.candidate.resolve())
    if not identity["launcher_executable"] or not identity["runtime_executable"]:
        raise SystemExit("candidate_binary_pair_unavailable")
    plan = {
        "schema": SCHEMA,
        "record_type": "plan",
        "candidate": {
            "revision": source_identity["git_commit"],
            "revision_attestation": "git_verified_clean_source",
            "git_commit": source_identity["git_commit"],
            "git_tree": source_identity["git_tree"],
            "source_worktree_clean": source_identity["worktree_clean"],
            "revision_matches_head": source_identity["revision_matches_head"],
            "launcher_sha256": identity["launcher_sha256"],
            "runtime_sha256": identity["runtime_sha256"],
            "pair_sha256": identity["pair_sha256"],
            "build": build,
        },
        "supervisor_sha256": harness.sha256_file(Path(__file__).resolve()),
        "exec_harness_sha256": harness.sha256_file(HARNESS_PATH),
        "model": args.model,
        "task_id": harness.TASK_ID,
        "fixture_sha256": harness.fixture_sha256(),
        "verifier_sha256": harness.sha256_file(harness.VERIFIER),
        "capture_phase": "durable_pre_model_io",
        "max_api_requests": harness.MAX_API_REQUESTS_PER_LANE,
        "max_runtime_seconds": harness.MAX_RUNTIME_SECONDS_PER_LANE,
        "key_accessed": False,
        "network_accessed": False,
        "product_metric_eligible": False,
    }
    if args.dry_run:
        print(json.dumps(plan, ensure_ascii=True, sort_keys=True))
        return 0
    if args.key_file is None or args.output is None:
        parser.error("--key-file and --output are required for a live canary")
    key = args.key_file.read_text(encoding="utf-8").strip()
    if not key:
        raise SystemExit("key_file_empty")
    with tempfile.TemporaryDirectory(prefix="codewhale-m4a-resume-") as temporary:
        root = Path(temporary)
        workspace = root / "workspace"
        state_root = root / "state"
        initial_snapshot = harness.initialize_fixture_workspace(workspace)
        initial_hash = harness.workspace_hash(initial_snapshot)
        db_path = state_root / "codewhale" / "state.db"
        start_command = exec_command(harness, args.candidate.resolve(), args.model, None)
        capture_env = harness.child_environment(key, state_root)
        run_id, pre_crash, crash = capture_and_kill(
            harness,
            start_command,
            capture_env,
            workspace,
            db_path,
            args.capture_timeout,
        )
        resume_env = harness.child_environment(key, state_root)
        key = ""
        resumed, resumed_owner = run_exec_with_lease_observation(
            harness,
            exec_command(harness, args.candidate.resolve(), args.model, run_id),
            resume_env,
            workspace,
            db_path,
            pre_crash["execution_epoch"],
        )
        resume_env.clear()
        verifier_input = harness.snapshot_workspace(workspace)
        verifier = harness.run_verifier(workspace)
        final_snapshot = harness.snapshot_workspace(workspace)
        changed = harness.changed_files(initial_snapshot, final_snapshot)
        completed = read_db_state(db_path)
        if completed is None:
            raise RuntimeError("completed_state_missing")
        completed_terminal = terminal_safe(resumed.stream.terminal)
        prefix_digest_after_reopen = read_event_prefix_digest(
            db_path, run_id, pre_crash["event_count"]
        )
        first_sequence_after_resume = (
            pre_crash["last_sequence"] + 1
            if completed["last_sequence"] > pre_crash["last_sequence"]
            else None
        )
        model_request_delta = (
            completed["model_request_count"] - pre_crash["model_request_count"]
        )
        tool_side_effect_delta = (
            completed["tool_side_effect_count"]
            - pre_crash["tool_side_effect_count"]
        )
        lease_reclaimed = bool(
            crash["lease_owner_pid_dead"]
            and resumed_owner is not None
            and resumed_owner["owner_id"] != pre_crash["lease_owner_id"]
            and resumed_owner["execution_epoch"] > pre_crash["execution_epoch"]
            and completed["execution_epoch"] > pre_crash["execution_epoch"]
            and not completed["lease_owner_id_present"]
            and completed["lease_owner_pid"] is None
        )
        unknown_billing = bool(
            completed_terminal.get("billing_unknown_attempts", 0) > 0
        )
        resume_ok = bool(
            resumed.returncode == 0
            and not resumed.timed_out
            and not resumed.spawn_error
            and resumed.stream.protocol_errors() == []
            and completed_terminal.get("status") == "completed"
            and completed_terminal.get("termination_reason") == "resolved"
            and verifier.get("passed") is True
            and verifier_input == final_snapshot
            and changed == harness.EXPECTED_CHANGED_FILES
            and completed["run_id"] == run_id
            and completed["terminal"]
            and completed["terminal_count"] == 1
            and completed["terminal_last"]
            and completed["sequences_contiguous"]
            and completed["event_ids_unique"]
            and completed["event_run_ids_match"]
            and completed["terminal_outcome_run_ids_match"]
            and completed["last_sequence"] > pre_crash["last_sequence"]
            and first_sequence_after_resume == pre_crash["last_sequence"] + 1
            and prefix_digest_after_reopen == pre_crash["event_digest"]
            and completed["unique_event_id_count"] == completed["event_count"]
            and model_request_delta
            == completed_terminal.get("api_request_count")
            and tool_side_effect_delta >= 0
            and lease_reclaimed
            and completed["lease_owner_pid"] is None
            and not completed["pending_model_in_flight"]
        )
        before_replay_hash = harness.workspace_hash(final_snapshot)
        replay_env = no_key_environment(harness, state_root)
        replayed = harness.run_exec_process(
            exec_command(harness, args.candidate.resolve(), args.model, run_id),
            replay_env,
            workspace,
        )
        replay_env.clear()
        replay_state = read_db_state(db_path)
        replay_snapshot = harness.snapshot_workspace(workspace)
        replay_terminal = terminal_safe(replayed.stream.terminal)
        stable_terminal_unchanged = stable_terminal_receipt(
            replay_terminal
        ) == stable_terminal_receipt(completed_terminal)
        replay_ok = bool(
            replay_state is not None
            and replayed.returncode == 0
            and not replayed.timed_out
            and not replayed.spawn_error
            and replayed.stream.protocol_errors() == []
            and stable_terminal_unchanged
            and replay_state["run_id"] == run_id
            and replay_state["event_count"] == completed["event_count"]
            and replay_state["last_sequence"] == completed["last_sequence"]
            and replay_state["event_digest"] == completed["event_digest"]
            and replay_state["terminal_count"] == 1
            and replay_state["tool_side_effect_count"]
            == completed["tool_side_effect_count"]
            and replay_state["tool_side_effect_digest"]
            == completed["tool_side_effect_digest"]
            and harness.workspace_hash(replay_snapshot) == before_replay_hash
        )
        recovery_record = {
            "run_id": run_id,
            "crash_phase": crash["phase"],
            "kill_mechanism": crash["kill_mechanism"],
            "same_run": completed["run_id"] == run_id,
            "last_committed_seq_before_crash": pre_crash["last_sequence"],
            "first_committed_seq_after_resume": first_sequence_after_resume,
            "event_count_before_crash": pre_crash["event_count"],
            "event_count_after_resume": completed["event_count"],
            "event_prefix_digest_before_crash": pre_crash["event_digest"],
            "event_prefix_digest_after_reopen": prefix_digest_after_reopen,
            "final_event_digest": completed["event_digest"],
            "unique_event_id_count": completed["unique_event_id_count"],
            "terminal_count": completed["terminal_count"],
            "model_request_delta": model_request_delta,
            "usage_delta": terminal_usage_delta(completed_terminal),
            "unknown_billing": unknown_billing,
            "tool_side_effect_count": {
                "before": pre_crash["tool_side_effect_count"],
                "after": completed["tool_side_effect_count"],
                "delta": tool_side_effect_delta,
            },
            "tool_side_effect_digest": {
                "before": pre_crash["tool_side_effect_digest"],
                "after": completed["tool_side_effect_digest"],
            },
            "lease_outcome": {
                "status": "reclaimed_dead_owner" if lease_reclaimed else "failed",
                "decision": "reclaim",
                "old_owner": {
                    "owner_id": pre_crash["lease_owner_id"],
                    "owner_pid": pre_crash["lease_owner_pid"],
                    "execution_epoch": pre_crash["execution_epoch"],
                    "pid_dead_after_kill": crash["lease_owner_pid_dead"],
                },
                "new_owner": resumed_owner,
                "execution_epoch_before": pre_crash["execution_epoch"],
                "execution_epoch_after": completed["execution_epoch"],
                "old_owner_pid_dead": crash["lease_owner_pid_dead"],
                "final_owner_released": not completed["lease_owner_id_present"]
                and completed["lease_owner_pid"] is None,
                "live_owner_rejection_evidence": {
                    "kind": "offline_process_and_store_tests",
                    "tests": [
                        "execution_epoch_fences_stale_leases_and_reclaims_dead_pid",
                        "concurrent_store_instances_allow_only_one_writer",
                    ],
                },
            },
            "no_key_replay": replay_ok,
            "workspace_revision_sha256": harness.workspace_hash(final_snapshot),
        }
        result = {
            **plan,
            "record_type": "resume_canary",
            "status": "passed" if resume_ok and replay_ok else "failed",
            "key_accessed": True,
            "network_accessed": True,
            "run_id": run_id,
            "recovery_record": recovery_record,
            "crash": crash,
            "resume": {
                "same_run": completed["run_id"] == run_id,
                "success": resume_ok,
                "process_exit_code": resumed.returncode,
                "wall_time_ms": resumed.wall_time_ms,
                "stream_contract_pass": resumed.stream.protocol_errors() == [],
                "terminal": completed_terminal,
                "event_count": completed["event_count"],
                "last_sequence": completed["last_sequence"],
                "first_sequence": first_sequence_after_resume,
                "event_digest": completed["event_digest"],
                "event_prefix_digest": prefix_digest_after_reopen,
                "unique_event_id_count": completed["unique_event_id_count"],
                "terminal_count": completed["terminal_count"],
                "verifier": verifier,
                "changed_files": changed,
                "workspace_revision_sha256": harness.workspace_hash(final_snapshot),
            },
            "no_key_terminal_replay": {
                "success": replay_ok,
                "process_exit_code": replayed.returncode,
                "stream_contract_pass": replayed.stream.protocol_errors() == [],
                "event_count_unchanged": replay_state is not None
                and replay_state["event_count"] == completed["event_count"],
                "last_sequence_unchanged": replay_state is not None
                and replay_state["last_sequence"] == completed["last_sequence"],
                "event_digest_unchanged": replay_state is not None
                and replay_state["event_digest"] == completed["event_digest"],
                "workspace_unchanged": harness.workspace_hash(replay_snapshot)
                == before_replay_hash,
                "terminal_accounting_unchanged": stable_terminal_unchanged,
                "new_events": 0
                if replay_state is not None
                and replay_state["event_count"] == completed["event_count"]
                else None,
            },
            "resume_success": resume_ok,
            "false_success": bool(
                completed_terminal.get("status") == "completed"
                and verifier.get("passed") is not True
            ),
            "recovery_contract_pass": resume_ok and replay_ok,
            "workspace_initial_sha256": initial_hash,
        }
    atomic_write(args.output.resolve(), result)
    print(
        json.dumps(
            {
                "schema": SCHEMA,
                "status": result["status"],
                "resume_success": result["resume_success"],
                "recovery_contract_pass": result["recovery_contract_pass"],
                "output_sha256": harness.sha256_file(args.output.resolve()),
            },
            sort_keys=True,
        )
    )
    return 0 if result["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
