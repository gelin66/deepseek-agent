#!/usr/bin/env python3
"""M8-M same-binary Chinese production-prompt successor evaluator.

The evaluator drives the current Run API v11 through ``codewhale app-server
--stdio`` and projects only canonical RuntimeEvent v17 / State v23 facts.  It
does not implement tools, an Agent loop, a verifier, request planning, pricing,
or retry policy.
"""

from __future__ import annotations

import argparse
from collections import Counter, defaultdict
from copy import deepcopy
from fractions import Fraction
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import time
from typing import Any, BinaryIO
import uuid


ROOT = Path(__file__).resolve().parents[1]
MANIFEST_PATH = ROOT / "eval/manifests/m8-m-prompt-successor-v1.json"
TEST_PATH = ROOT / "scripts/test-eval-m8m-prompt-successor.py"
M7E_PATH = ROOT / "scripts/eval-m7e-thinking.py"
CANDIDATE_PROMPT_PATH = ROOT / "eval/fixtures/m8-d-prompt/v1/constitution.md"
SINGLE_TASK_SOURCE = ROOT / "eval/manifests/m7-a2-agent-convergence-ab-v1.json"
WRITER_TASK_SOURCE = ROOT / "eval/manifests/m6-b1-writer-benefit-ab-v3.json"
SCHEMA = "codewhale.eval.m8-m-prompt-successor.v1"
RESULT_SCHEMA = "codewhale.eval.m8-m-prompt-successor-result.v1"
VARIANTS = ("baseline", "candidate")
RUN_API = 11
EVENT_API = 17
STATE_SCHEMA = 23
REASONING_EFFORT = "high"
MODEL = "deepseek-v4-pro"
JOURNAL_SCHEMA = "codewhale.eval.m8-m-prompt-successor-journal.v1"
ZERO_HASH = "sha256:" + ("0" * 64)
JOURNAL_FAULTS = (
    "after_suite_plan_fsync_kill",
    "terminal_mid_write_kill",
    "after_terminal_fsync_kill",
    "after_reopen_fsync_kill",
    "after_verifier_fsync_kill",
    "before_arm_observation_kill",
)
LIFECYCLE_KINDS = (
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
    "agent_cleanup_prepared",
    "agent_cleanup_committed",
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


def load_module(path: Path, name: str) -> Any:
    spec = importlib.util.spec_from_file_location(name, path)
    require(spec is not None and spec.loader is not None, f"{name}_unavailable")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


M7E = load_module(M7E_PATH, "codewhale_m8m_m7e_projection")
M7E.RUN_API = RUN_API
M7E.EVENT_API = EVENT_API
M7E.STATE_SCHEMA = STATE_SCHEMA
EVALUATION_ERRORS = (EvaluationError, M7E.EvaluationError)


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


def canonical_hash(value: Any) -> str:
    return sha256_bytes(canonical_bytes(value))


def file_hash(path: Path) -> str:
    return sha256_bytes(path.read_bytes())


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


def journal_record_core(
    sequence: int,
    previous_record_sha256: str,
    payload: dict[str, Any],
) -> dict[str, Any]:
    return {
        "schema": JOURNAL_SCHEMA,
        "sequence": sequence,
        "previous_record_sha256": previous_record_sha256,
        "payload": payload,
    }


def encode_journal_record(
    sequence: int,
    previous_record_sha256: str,
    payload: dict[str, Any],
) -> tuple[bytes, str]:
    core = journal_record_core(sequence, previous_record_sha256, payload)
    record_sha256 = canonical_hash(core)
    value = {**core, "record_sha256": record_sha256}
    return canonical_bytes(value) + b"\n", record_sha256


class Journal:
    def __init__(self, path: Path, stream: BinaryIO) -> None:
        self.path = path
        self.stream = stream
        self.sequence = 0
        self.previous_record_sha256 = ZERO_HASH

    @classmethod
    def claim(cls, path: Path) -> "Journal":
        require(path.is_absolute(), "output_must_be_absolute")
        require(path.parent.is_dir(), "output_parent_missing")
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
        try:
            os.fchmod(descriptor, 0o600)
            fsync_directory(path.parent)
            stream = os.fdopen(descriptor, "wb", buffering=0)
        except BaseException:
            os.close(descriptor)
            raise
        return cls(path, stream)

    def __enter__(self) -> "Journal":
        return self

    def __exit__(self, *_: object) -> None:
        self.stream.close()

    def emit(
        self,
        payload: dict[str, Any],
        *,
        fault: str | None = None,
    ) -> str:
        encoded, record_sha256 = encode_journal_record(
            self.sequence + 1,
            self.previous_record_sha256,
            payload,
        )
        if fault == "mid_write_kill":
            write_all(
                self.stream.fileno(),
                encoded[: max(1, len(encoded) // 2)],
            )
            os.kill(os.getpid(), signal.SIGKILL)
        write_all(self.stream.fileno(), encoded)
        self.stream.flush()
        os.fsync(self.stream.fileno())
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
    for index, line in enumerate(parts, start=1):
        require(bool(line), "journal_blank_record")
        try:
            record = json.loads(line)
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise EvaluationError("journal_record_invalid") from error
        require(
            isinstance(record, dict)
            and record.get("schema") == JOURNAL_SCHEMA
            and record.get("sequence") == index
            and record.get("previous_record_sha256") == previous
            and isinstance(record.get("payload"), dict),
            "journal_chain_invalid",
        )
        core = {
            key: record[key]
            for key in (
                "schema",
                "sequence",
                "previous_record_sha256",
                "payload",
            )
        }
        require(
            record.get("record_sha256") == canonical_hash(core),
            "journal_hash_invalid",
        )
        previous = record["record_sha256"]
        records.append(record)
    return {
        "records": records,
        "partial_tail_bytes": len(tail),
        "partial_tail_sha256": sha256_bytes(tail) if tail else None,
        "file_sha256": sha256_bytes(raw),
    }


def journal_payload_types(audit: dict[str, Any]) -> list[str]:
    return [
        record["payload"].get("record_type")
        for record in audit["records"]
    ]


def journal_fault_contract_hash() -> str:
    return canonical_hash(
        {
            "faults": list(JOURNAL_FAULTS),
            "durable_order": [
                "suite_plan",
                "terminal_snapshot",
                "sqlite_reopen_snapshot",
                "verifier_snapshot",
                "arm_observation",
            ],
            "maximum_reruns": 0,
        }
    )


def run_journal_fault_child(fault: str, output: Path) -> int:
    require(fault in JOURNAL_FAULTS, "journal_fault_unknown")
    with Journal.claim(output) as journal:
        journal.emit(
            {
                "record_type": "suite_plan",
                "key_accessed": False,
                "network_accessed": False,
                "product_metric_eligible": False,
            }
        )
        if fault == "after_suite_plan_fsync_kill":
            os.kill(os.getpid(), signal.SIGKILL)
        journal.emit(
            {
                "record_type": "terminal_snapshot",
                "canonical_store_facts": {"run": "fixture"},
                "key_accessed": False,
                "network_accessed": False,
                "product_metric_eligible": False,
            },
            fault=(
                "mid_write_kill"
                if fault == "terminal_mid_write_kill"
                else None
            ),
        )
        if fault == "after_terminal_fsync_kill":
            os.kill(os.getpid(), signal.SIGKILL)
        journal.emit(
            {
                "record_type": "sqlite_reopen_snapshot",
                "canonical_store_facts": {"run": "fixture"},
                "key_accessed": False,
                "network_accessed": False,
                "product_metric_eligible": False,
            }
        )
        if fault == "after_reopen_fsync_kill":
            os.kill(os.getpid(), signal.SIGKILL)
        journal.emit(
            {
                "record_type": "verifier_snapshot",
                "external_verifier": {"passed": True},
                "key_accessed": False,
                "network_accessed": False,
                "product_metric_eligible": False,
            }
        )
        if fault == "after_verifier_fsync_kill":
            os.kill(os.getpid(), signal.SIGKILL)
        if fault == "before_arm_observation_kill":
            os.kill(os.getpid(), signal.SIGKILL)
        journal.emit(
            {
                "record_type": "arm_observation",
                "key_accessed": False,
                "network_accessed": False,
                "product_metric_eligible": False,
            }
        )
    return 0


def run_journal_fault_matrix() -> list[dict[str, Any]]:
    results: list[dict[str, Any]] = []
    with tempfile.TemporaryDirectory(
        prefix="codewhale-m8m-journal-faults-"
    ) as raw:
        root = Path(raw)
        for fault in JOURNAL_FAULTS:
            output = root / f"{fault}.jsonl"
            completed = subprocess.run(
                [
                    sys.executable,
                    str(Path(__file__).resolve()),
                    "journal-fault-child",
                    "--fault",
                    fault,
                    "--output",
                    str(output),
                ],
                cwd=ROOT,
                env=M7E.safe_env(),
                stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=30,
                check=False,
            )
            require(
                completed.returncode == -signal.SIGKILL
                and not completed.stdout
                and not completed.stderr,
                "journal_fault_process_invalid",
                {"fault": fault, "returncode": completed.returncode},
            )
            audit = read_journal(output, allow_partial_tail=True)
            record_types = journal_payload_types(audit)
            require(
                record_types
                and record_types[0] == "suite_plan"
                and "arm_observation" not in record_types,
                "journal_fault_derived_observation_committed",
                {"fault": fault, "record_types": record_types},
            )
            if fault == "terminal_mid_write_kill":
                require(
                    record_types == ["suite_plan"]
                    and audit["partial_tail_bytes"] > 0,
                    "journal_fault_partial_tail_invalid",
                )
            elif fault in {
                "after_reopen_fsync_kill",
                "after_verifier_fsync_kill",
                "before_arm_observation_kill",
            }:
                require(
                    "terminal_snapshot" in record_types
                    and "sqlite_reopen_snapshot" in record_types,
                    "journal_fault_reopen_order_invalid",
                )
            results.append(
                {
                    "fault": fault,
                    "record_types": record_types,
                    "partial_tail_bytes": audit["partial_tail_bytes"],
                    "file_sha256": audit["file_sha256"],
                    "passed": True,
                }
            )
    return results


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


def resolve_manifest_file(path: Path, seen: set[Path] | None = None) -> dict[str, Any]:
    resolved_path = path.resolve()
    manifest_root = (ROOT / "eval/manifests").resolve()
    require(
        resolved_path.parent == manifest_root and resolved_path.suffix == ".json",
        "manifest_path_invalid",
    )
    visited = set() if seen is None else set(seen)
    require(resolved_path not in visited, "manifest_cycle")
    visited.add(resolved_path)
    overlay = load_json(resolved_path, "manifest_unavailable")
    base_ref = overlay.get("base_manifest")
    if base_ref is None:
        return overlay
    require(isinstance(base_ref, dict), "base_manifest_identity_mismatch")
    relative = base_ref.get("path")
    require(isinstance(relative, str), "base_manifest_identity_mismatch")
    base_path = (ROOT / relative).resolve()
    require(
        base_path.parent == manifest_root
        and base_ref.get("sha256") == file_hash(base_path),
        "base_manifest_identity_mismatch",
    )
    manifest = deepcopy(resolve_manifest_file(base_path, visited))
    manifest["schema"] = overlay["schema"]
    manifest["status"] = overlay["status"]
    manifest["date"] = overlay["date"]
    manifest["claim"].update(overlay["claim"])
    manifest.pop("prior_attempt", None)
    if "suite_lineage" in overlay:
        manifest["suite_lineage"] = overlay["suite_lineage"]
    if "prior_attempts" in overlay:
        manifest["prior_attempts"] = overlay["prior_attempts"]
    for section in (
        "source_identity",
        "experiment",
        "admission",
        "output",
        "prompt_treatment",
    ):
        if section in overlay:
            manifest[section].update(overlay[section])
    for section in (
        "binary_identity",
        "official_protocol_review",
        "resources",
        "acceptance",
    ):
        if section in overlay:
            manifest[section] = deepcopy(overlay[section])
    if "offline_gates" in overlay:
        manifest["offline_gates"] = overlay["offline_gates"]
    manifest["frozen_hashes"] = overlay["frozen_hashes"]
    return manifest


def resolved_manifest() -> dict[str, Any]:
    manifest = resolve_manifest_file(MANIFEST_PATH)
    require(manifest.get("schema") == SCHEMA, "manifest_schema_mismatch")
    return manifest


def assemble_tasks(
    manifest: dict[str, Any],
) -> dict[str, Any]:
    single = load_json(SINGLE_TASK_SOURCE, "single_task_source_unavailable")
    writer = load_json(WRITER_TASK_SOURCE, "writer_task_source_unavailable")
    sources = manifest["task_sources"]
    require(
        file_hash(SINGLE_TASK_SOURCE) == sources["single_readonly"]["sha256"],
        "single_task_source_hash_mismatch",
    )
    require(
        file_hash(WRITER_TASK_SOURCE) == sources["writer"]["sha256"],
        "writer_task_source_hash_mismatch",
    )
    tasks: dict[str, dict[str, Any]] = {}
    for mapping in sources["tasks"]:
        task_id = mapping["id"]
        source_task_id = mapping["source_task_id"]
        if mapping["source"] == "single_readonly":
            task = dict(single["tasks"][source_task_id])
        elif mapping["source"] == "writer":
            task = dict(writer["tasks"][source_task_id])
            task.update(
                {
                    "acceptance_id": f"m8m-{task_id}",
                    "evidence_policy": (
                        "failed_write_pass" if task_id == "w3" else "latest_pass"
                    ),
                    "max_depth": 1,
                    "max_concurrent_children": 1,
                    "child_expectation": "exactly_one_isolated_writer",
                }
            )
        else:
            raise EvaluationError("task_source_kind_invalid")
        task.update(
            {
                "id": task_id,
                "source_task_id": source_task_id,
                "lane": mapping["lane"],
                "fixture_tree_sha256": mapping["fixture_tree_sha256"],
            }
        )
        tasks[task_id] = task
    return {
        "tasks": tasks,
        "tool_policy": manifest["tool_policy"],
    }


def materialize_fixture(
    tasks: dict[str, Any],
    task_id: str,
    destination: Path,
) -> str:
    frozen = tasks["tasks"][task_id]
    require(
        M7E.fixture_hash(tasks, task_id) == frozen["fixture_tree_sha256"],
        "fixture_hash_mismatch",
    )
    shutil.copytree(M7E.fixture_path(tasks, task_id), destination)
    writer = frozen["lane"] == "explicit_writer"
    timestamp = "2026-07-21T00:00:00Z" if writer else "2026-07-22T00:00:00Z"
    message = (
        f"M6-B1 frozen fixture {frozen['source_task_id']}"
        if writer
        else f"M7-A frozen fixture {M7E.fixture_path(tasks, task_id).name}"
    )
    environment = {
        **M7E.safe_env(),
        "GIT_AUTHOR_DATE": timestamp,
        "GIT_COMMITTER_DATE": timestamp,
    }
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
            message,
        ],
    )
    for command in commands:
        result = M7E.run_command(
            command,
            cwd=destination,
            environment=environment,
        )
        require(result.returncode == 0, "fixture_git_init_failed")
    base = M7E.git_output("rev-parse", "HEAD", cwd=destination)
    require(
        base == frozen["fixture_base_commit"]
        and M7E.git_output(
            "status",
            "--porcelain=v1",
            "--untracked-files=all",
            cwd=destination,
        )
        == ""
        and M7E.git_output("symbolic-ref", "-q", "HEAD", cwd=destination)
        == "refs/heads/main",
        "fixture_git_identity_mismatch",
    )
    return base


def probe_fixture_identities(tasks: dict[str, Any]) -> dict[str, Any]:
    with tempfile.TemporaryDirectory(prefix="codewhale-m8m-fixtures-") as raw:
        root = Path(raw)
        return {
            task_id: {
                "base_commit": materialize_fixture(
                    tasks, task_id, root / task_id
                ),
                "tree_sha256": M7E.fixture_hash(tasks, task_id),
            }
            for task_id in tasks["tasks"]
        }


def load_manifest(*, frozen: bool) -> tuple[dict[str, Any], dict[str, Any]]:
    manifest = resolved_manifest()
    source = manifest.get("source_identity", {})
    require(
        source.get("run_api") == RUN_API
        and source.get("runtime_event") == EVENT_API
        and source.get("state_schema") == STATE_SCHEMA
        and source.get("exec_stream") == 3,
        "protocol_identity_invalid",
    )
    treatment = manifest.get("prompt_treatment", {})
    require(
        file_hash(CANDIDATE_PROMPT_PATH)
        == treatment.get("candidate", {}).get("sha256"),
        "candidate_prompt_hash_mismatch",
    )
    baseline = (ROOT / treatment["baseline"]["source"]).read_text(encoding="utf-8")
    candidate = CANDIDATE_PROMPT_PATH.read_text(encoding="utf-8")
    require(
        treatment["baseline"]["required_fragment"] in baseline
        and treatment["candidate"]["required_fragment"] in candidate
        and treatment["candidate"]["forbidden_fragment"] not in candidate,
        "prompt_fragment_contract_invalid",
    )
    require(
        exact_candidate_delta(baseline, candidate),
        "candidate_prompt_delta_invalid",
    )
    experiment = manifest.get("experiment", {})
    require(
        experiment.get("variants") == list(VARIANTS)
        and experiment.get("runs_per_variant_task") == 3
        and experiment.get("formal_pairs") == 15
        and experiment.get("formal_arms") == 30
        and experiment.get("maximum_reruns") == 0,
        "experiment_shape_invalid",
    )
    tasks = assemble_tasks(manifest)
    require(
        list(tasks["tasks"]) == ["t1", "t3", "t5", "w1", "w3"],
        "task_order_invalid",
    )
    for task_id, task in tasks["tasks"].items():
        require(
            M7E.fixture_hash(tasks, task_id) == task["fixture_tree_sha256"],
            "fixture_hash_mismatch",
            {"task_id": task_id},
        )
    if frozen:
        hashes = manifest.get("frozen_hashes", {})
        require(
            hashes.get("harness_sha256") == file_hash(Path(__file__).resolve())
            and hashes.get("harness_test_sha256") == file_hash(TEST_PATH)
            and hashes.get("schedule_sha256")
            == canonical_hash(formal_schedule(manifest))
            and hashes.get("manifest_content_sha256_excluding_frozen_hashes")
            == manifest_content_hash(manifest),
            "frozen_hash_mismatch",
        )
    return manifest, tasks


def exact_candidate_delta(baseline: str, candidate: str) -> bool:
    old = (
        "1. 修改前读取当前作用域的仓库规则；\n"
        "2. 检查足够的代码、调用链和当前行为，确定真实责任边界；\n"
        "3. 在安全且成本合理时复现问题；\n"
        "4. 做最小而完整的修改，保留无关工作；\n"
        "5. 运行与风险相称的验证，并检查最终 diff。"
    )
    new = (
        "修改前读取当前作用域的仓库规则。之后根据尚未解决且会改变下一步决策的事实缺口选择调查、\n"
        "修改、验证或完成；每次获得新证据后重新判断。"
    )
    return old in baseline and baseline.replace(old, new) == candidate


def formal_schedule(manifest: dict[str, Any]) -> list[dict[str, Any]]:
    schedule: list[dict[str, Any]] = []
    pair_index = 0
    position = 0
    for run_index, task_order in enumerate(
        manifest["experiment"]["round_order"], start=1
    ):
        for task_id in task_order:
            pair_index += 1
            order = (
                ["baseline", "candidate"]
                if pair_index % 2
                else ["candidate", "baseline"]
            )
            for pair_position, variant in enumerate(order, start=1):
                position += 1
                schedule.append(
                    {
                        "schedule_position": position,
                        "pair_index": pair_index,
                        "pair_position": pair_position,
                        "pair_order": order,
                        "run_index": run_index,
                        "task_id": task_id,
                        "variant": variant,
                    }
                )
    require(
        len(schedule) == manifest["experiment"]["formal_arms"],
        "schedule_size_invalid",
    )
    return schedule


def verifier_spec(task_id: str) -> dict[str, Any]:
    name = f"m8m-{task_id}-exact"
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
                    "timeout_ms": 600000,
                }
            ]
        },
    }


def writer_arguments(manifest: dict[str, Any], task: dict[str, Any]) -> dict[str, Any]:
    return {
        "type": "implementer",
        "workspace_access": "isolated_write",
        "allowed_paths": task["allowed_paths"],
        "fork_context": False,
        "allowed_tools": manifest["tool_policy"]["writer_child_tools"],
        "max_steps": 7,
        "max_depth": 0,
        "wall_time_secs": 180,
        "expected_artifact": task["expected_artifact"],
    }


def task_definition(
    manifest: dict[str, Any],
    tasks: dict[str, Any],
    task_id: str,
) -> dict[str, Any]:
    task = tasks["tasks"][task_id]
    objective = task["objective"]
    if task["lane"] == "explicit_writer":
        arguments = json.dumps(
            writer_arguments(manifest, task),
            ensure_ascii=False,
            sort_keys=True,
            separators=(",", ":"),
        )
        objective += (
            "\n\n处理协议由实际工具目录决定，且是验收的一部分：根 Agent 只读调查和审查，"
            "不得调用写工具或 verifier；必须且只能调用一次 agent。除 prompt 外参数必须精确等于 "
            f"{arguments}；prompt 必须完整转交当前任务。Writer 集成后根 Agent 只读审查并提出完成，"
            "最终只认 Host 冻结 verifier。"
        )
    return {
        "objective": objective,
        "constraints": task["constraints"],
        "non_goals": task["non_goals"],
        "acceptance": [
            {
                "kind": "verifier",
                "id": task["acceptance_id"],
                "description": f"{task['name']} 的冻结确定性验收",
                "evidence_policy": task["evidence_policy"],
                "verifier": verifier_spec(task_id),
            }
        ],
    }


def start_envelope(
    manifest: dict[str, Any],
    tasks: dict[str, Any],
    task_id: str,
    workspace: Path,
    request_id: str,
) -> dict[str, Any]:
    resources = manifest["resources"]
    task = tasks["tasks"][task_id]
    writer = task["lane"] == "explicit_writer"
    return {
        "schema_version": RUN_API,
        "request_id": request_id,
        "command": {
            "kind": "start",
            "task": task_definition(manifest, tasks, task_id),
            "workspace": str(workspace.resolve()),
            "model": manifest["experiment"]["model"],
            "reasoning_effort": manifest["experiment"]["reasoning_effort"],
            "max_output_tokens": resources["max_output_tokens_per_request"],
            "max_api_requests": resources["max_physical_api_attempts_per_arm"],
            "streaming": resources["streaming"],
            "tool_policy": {
                "enabled": True,
                "allowed": manifest["tool_policy"]["root_tools"],
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
                "write_execution_mode": "isolated_writer" if writer else "root",
                "auto_approve": resources["auto_approve"],
                "trust_mode": resources["trust_mode"],
                "allow_sandbox_elevation": resources["allow_sandbox_elevation"],
                "interactive": resources["interactive"],
                "sandbox": resources["sandbox"],
            },
        },
    }


def event_records(
    events_by_actor: list[tuple[str, list[dict[str, Any]]]],
    kind: str,
) -> list[dict[str, Any]]:
    records: list[dict[str, Any]] = []
    for actor, events in events_by_actor:
        for stored in events:
            if M7E.event_kind(stored) == kind:
                records.append(
                    {
                        "actor": actor,
                        "sequence": stored.get("sequence"),
                        "event": stored["event"],
                    }
                )
    return records


def exact_tool_success(outcome: dict[str, Any]) -> bool:
    return bool(
        outcome.get("invocation") == "accepted"
        and outcome.get("transport") == "succeeded"
        and outcome.get("operation") == "succeeded"
        and outcome.get("retry") == "not_needed"
        and outcome.get("failure_code") is None
    )


def tool_metrics(
    root_events: list[dict[str, Any]],
    child_ledgers: list[list[dict[str, Any]]],
    verified_success: bool,
) -> dict[str, Any]:
    actors = [("root", root_events)] + [
        (f"child-{index}", events)
        for index, events in enumerate(child_ledgers, start=1)
    ]
    outcomes = event_records(actors, "tool_outcome_committed")
    values: list[dict[str, Any]] = []
    for record in outcomes:
        event = record["event"]
        outcome = event.get("outcome", {})
        values.append(
            {
                "actor": record["actor"],
                "sequence": record["sequence"],
                "name": event.get("name"),
                "success": exact_tool_success(outcome),
                "failure_code": outcome.get("failure_code"),
                "side_effect": outcome.get("side_effect"),
            }
        )
    first = values[0] if values else None
    edits = [
        value for value in values if value["name"] in {"apply_patch", "edit_file"}
    ]
    failed = [value for value in values if not value["success"]]
    recovered = bool(failed and verified_success and any(value["success"] for value in values))
    return {
        "count": len(values),
        "first_tool_correct": bool(first and first["success"]),
        "first_tool_name": first["name"] if first else None,
        "first_edit_success": bool(edits and edits[0]["success"]),
        "edit_attempts": len(edits),
        "failures": len(failed),
        "failure_codes": dict(
            sorted(
                Counter(
                    value["failure_code"]
                    for value in failed
                    if value["failure_code"] is not None
                ).items()
            )
        ),
        "recovered_after_failure": recovered,
    }


def prompt_signature(
    request: dict[str, Any],
    variant: str,
    baseline_text: str,
    candidate_text: str,
) -> dict[str, Any]:
    prompt = request.get("system_prompt")
    require(isinstance(prompt, dict), "system_prompt_missing")
    blocks = prompt.get("blocks")
    require(isinstance(blocks, list) and blocks, "system_prompt_blocks_missing")
    require(all(isinstance(block, dict) for block in blocks), "prompt_block_invalid")
    expected = candidate_text.strip() if variant == "candidate" else baseline_text.strip()
    first_text = blocks[0].get("text")
    require(isinstance(first_text, str), "stable_prompt_text_missing")
    prefix_valid = first_text.startswith(expected)
    suffix = first_text[len(expected) :] if prefix_valid else ""
    return {
        "expected_constitution_sha256": sha256_bytes(expected.encode("utf-8")),
        "prefix_valid": prefix_valid and suffix.startswith("\n\n"),
        "block_count": len(blocks),
        "cache_controls": [block.get("cache_control") for block in blocks],
        "stable_block_sha256": sha256_bytes(first_text.encode("utf-8")),
        "stable_suffix_sha256": sha256_bytes(suffix.encode("utf-8")),
        "remaining_blocks_sha256": canonical_hash(blocks[1:]),
        "whole_prompt_sha256": canonical_hash(prompt),
    }


def request_projection(
    variant: str,
    root_events: list[dict[str, Any]],
    child_ledgers: list[list[dict[str, Any]]],
    baseline_text: str,
    candidate_text: str,
) -> dict[str, Any]:
    requests = [
        ("root", event)
        for event in M7E.event_values(root_events, "model_request_prepared")
    ]
    for index, events in enumerate(child_ledgers, start=1):
        requests.extend(
            (f"child-{index}", event)
            for event in M7E.event_values(events, "model_request_prepared")
        )
    fingerprints: list[dict[str, Any]] = []
    for actor_label, event in requests:
        request = event.get("request", {})
        semantic_messages, generation_count = M7E.normalized_request_messages_hash(
            request.get("messages")
        )
        fingerprints.append(
            {
                "actor_label": actor_label,
                "actor": request.get("actor"),
                "model": request.get("model"),
                "reasoning_effort": request.get("reasoning_effort"),
                "semantic_messages_sha256": semantic_messages,
                "task_generation_count": generation_count,
                "tools_sha256": canonical_hash(request.get("tools")),
                "max_output_tokens": request.get("max_output_tokens"),
                "streaming": request.get("streaming"),
                "prompt": prompt_signature(
                    request, variant, baseline_text, candidate_text
                ),
            }
        )
    root = [value for value in fingerprints if value["actor_label"] == "root"]
    return {
        "count": len(fingerprints),
        "valid": bool(root)
        and all(
            value["model"] == "deepseek-v4-pro"
            and value["reasoning_effort"] == REASONING_EFFORT
            and value["prompt"]["prefix_valid"]
            for value in fingerprints
        ),
        "first_root": root[0] if root else None,
        "first_by_actor": {
            actor: next(value for value in fingerprints if value["actor_label"] == actor)
            for actor in sorted({value["actor_label"] for value in fingerprints})
        },
        "fingerprints": fingerprints,
    }


def accounting_projection(run: dict[str, Any]) -> dict[str, Any]:
    accounting = run.get("accounting")
    require(isinstance(accounting, dict), "accounting_missing")
    usage = accounting.get("usage")
    root = accounting.get("root")
    child = accounting.get("child")
    surface_usage = accounting.get("surface_usage")
    require(
        isinstance(usage, dict)
        and isinstance(root, dict)
        and isinstance(child, dict)
        and isinstance(surface_usage, list),
        "accounting_shape_invalid",
    )
    started = root.get("started", 0) + child.get("started", 0)
    completed = root.get("completed", 0) + child.get("completed", 0)
    in_flight = root.get("in_flight", 0) + child.get("in_flight", 0)
    usage_fields = (
        "input_tokens",
        "output_tokens",
        "cache_hit_tokens",
        "cache_miss_tokens",
        "cache_write_tokens",
        "reasoning_tokens",
        "reasoning_replay_tokens",
    )
    surface_usage_sum = {
        name: sum(
            bucket.get("usage", {}).get(name, 0)
            for bucket in surface_usage
            if isinstance(bucket, dict)
        )
        for name in usage_fields
    }
    surface_cost = sum(
        bucket.get("cost_nanousd", 0)
        for bucket in surface_usage
        if isinstance(bucket, dict)
    )
    surface_response_count = sum(
        bucket.get("response_count", 0)
        for bucket in surface_usage
        if isinstance(bucket, dict)
    )
    surface_usage_response_count = sum(
        bucket.get("usage_response_count", 0)
        for bucket in surface_usage
        if isinstance(bucket, dict)
    )
    surface_identity_valid = bool(surface_usage) and all(
        isinstance(bucket, dict)
        and bucket.get("surface") == "standard_chat"
        and bucket.get("model") == MODEL
        and isinstance(bucket.get("response_count"), int)
        and bucket.get("response_count", 0) > 0
        and bucket.get("usage_response_count") == bucket.get("response_count")
        for bucket in surface_usage
    )
    totals_valid = (
        surface_usage_sum == usage
        and surface_cost == accounting.get("cost_nanousd")
    )
    response_counts_valid = bool(
        started > 0
        and started == completed
        and in_flight == 0
        and surface_response_count == started
        and surface_usage_response_count == started
    )
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
        and surface_identity_valid
        and totals_valid
        and response_counts_valid
    )
    return {
        "valid": complete,
        "sealed": accounting.get("sealed"),
        "complete": accounting.get("complete"),
        "usage_complete": accounting.get("usage_complete"),
        "billing_unknown": accounting.get("billing_unknown"),
        "billing_unknown_attempts": accounting.get("billing_unknown_attempts"),
        "unpriced": accounting.get("unpriced"),
        "hard_request_limit": accounting.get("hard_request_limit"),
        "requests": {
            "started": started,
            "completed": completed,
            "in_flight": in_flight,
            "root_started": root.get("started"),
            "child_started": child.get("started"),
        },
        "surface_responses": {
            "responses": surface_response_count,
            "usage_responses": surface_usage_response_count,
            "valid": response_counts_valid,
        },
        "transport_retries": accounting.get("transport_retries"),
        "runtime_retries": accounting.get("runtime_retries"),
        "usage": usage,
        "root_usage": run.get("usage"),
        "usage_source": "accounting.aggregate",
        "surface_usage": surface_usage,
        "surface_identity_valid": surface_identity_valid,
        "surface_totals_valid": totals_valid,
        "cost_nanousd": accounting.get("cost_nanousd"),
    }


def bind_current_child_contract(
    root_events: list[dict[str, Any]],
    children: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    starts = M7E.event_values(root_events, "child_started")
    tasks = {
        event.get("task", {}).get("task_id"): event.get("task", {})
        for event in M7E.event_values(root_events, "agent_task_prepared")
    }
    require(len(starts) == len(children), "child_projection_count_mismatch")
    projected: list[dict[str, Any]] = []
    for start, child in zip(starts, children, strict=True):
        task = tasks.get(start.get("task_id"))
        require(isinstance(task, dict), "child_task_contract_missing")
        workspace = task.get("workspace")
        tool_policy = task.get("tool_policy")
        require(
            isinstance(workspace, dict) and isinstance(tool_policy, dict),
            "child_task_contract_invalid",
        )
        value = dict(child)
        value["workspace_access"] = workspace.get("access")
        value["allowed_tools"] = tool_policy.get("allowed")
        projected.append(value)
    return projected


def current_child_projection(
    client: Any,
    root_events: list[dict[str, Any]],
    deadline: float,
    suffix: str,
) -> tuple[list[dict[str, Any]], list[list[dict[str, Any]]]]:
    children, ledgers = M7E.child_projection(
        client, root_events, deadline, suffix
    )
    return bind_current_child_contract(root_events, children), ledgers


def child_valid(
    manifest: dict[str, Any],
    task: dict[str, Any],
    children: list[dict[str, Any]],
) -> bool:
    expectation = task["child_expectation"]
    if expectation in {"forbidden", "zero"}:
        return not children
    if len(children) != 1:
        return False
    child = children[0]
    if expectation == "exactly_one_read_only":
        allowed = manifest["tool_policy"]["readonly_child_tools"]
        return bool(
            child["terminal"]["state"] == "completed"
            and child["workspace_access"] == "read_only"
            and child["allowed_tools"] == allowed
            and all(name in allowed for name in child["tool_names"])
        )
    if expectation == "exactly_one_isolated_writer":
        allowed = manifest["tool_policy"]["writer_child_tools"]
        return bool(
            child["terminal"]["state"] == "completed"
            and child["workspace_access"] == "isolated_write"
            and child["allowed_tools"] == allowed
            and all(name in allowed for name in child["tool_names"])
        )
    return False


def writer_lifecycle(
    manifest: dict[str, Any],
    task: dict[str, Any],
    root_events: list[dict[str, Any]],
    children: list[dict[str, Any]],
    workspace: Path,
) -> dict[str, Any]:
    if task["lane"] != "explicit_writer":
        return {"required": False, "valid": True}
    counts = Counter(M7E.event_kind(event) for event in root_events)
    positions = {
        kind: [
            index
            for index, event in enumerate(root_events)
            if M7E.event_kind(event) == kind
        ]
        for kind in LIFECYCLE_KINDS
    }
    required_once = all(counts[kind] == 1 for kind in LIFECYCLE_KINDS)
    ordered = required_once and [
        positions[kind][0] for kind in LIFECYCLE_KINDS
    ] == sorted(positions[kind][0] for kind in LIFECYCLE_KINDS)
    cleanup = M7E.event_values(root_events, "agent_cleanup_committed")
    cleanup_status = (
        cleanup[0].get("result", {}).get("status")
        if len(cleanup) == 1
        else None
    )
    tasks = M7E.event_values(root_events, "agent_task_prepared")
    prepared_task = tasks[0].get("task", {}) if len(tasks) == 1 else {}
    workspace_assignment = (
        prepared_task.get("workspace", {})
        if isinstance(prepared_task, dict)
        else {}
    )
    root_tools = [
        event.get("invocation", {}).get("name")
        for event in M7E.event_values(root_events, "tool_prepared")
    ]
    root_authority = bool(
        root_tools.count("agent") == 1
        and all(name in manifest["tool_policy"]["writer_root_tools"] for name in root_tools)
    )
    worktrees = M7E.git_output("worktree", "list", "--porcelain", cwd=workspace)
    writer_refs = M7E.git_output(
        "for-each-ref",
        "--format=%(refname)",
        "refs/heads/codewhale/writer/",
        cwd=workspace,
    )
    root_status_clean = (
        M7E.git_output(
            "status",
            "--porcelain=v1",
            "--untracked-files=all",
            cwd=workspace,
        )
        == ""
    )
    leak_free = (
        worktrees.count("worktree ") == 1
        and writer_refs == ""
        and root_status_clean
    )
    valid = bool(
        len(children) == 1
        and children[0]["terminal"]["state"] == "completed"
        and required_once
        and ordered
        and counts["agent_integration_failed"] == 0
        and cleanup_status in {"removed", "already_absent"}
        and workspace_assignment.get("access") == "isolated_write"
        and workspace_assignment.get("base_commit")
        == task["fixture_base_commit"]
        and workspace_assignment.get("allowed_paths") == task["allowed_paths"]
        and root_authority
        and leak_free
    )
    return {
        "required": True,
        "valid": valid,
        "event_counts": {kind: counts[kind] for kind in LIFECYCLE_KINDS},
        "integration_failures": counts["agent_integration_failed"],
        "ordered": ordered,
        "cleanup_status": cleanup_status,
        "root_authority": root_authority,
        "root_status_clean": root_status_clean,
        "leak_free": leak_free,
    }


def observed_changed_files(
    workspace: Path,
    base_commit: str,
    lane: str,
) -> list[str]:
    if lane != "explicit_writer":
        return M7E.changed_files(workspace)
    head = M7E.git_output("rev-parse", "HEAD", cwd=workspace)
    require(head != base_commit, "writer_head_not_advanced")
    committed = M7E.git_output(
        "diff",
        "--name-only",
        "--no-renames",
        base_commit,
        head,
        cwd=workspace,
    ).splitlines()
    untracked = M7E.git_output(
        "ls-files",
        "--others",
        "--exclude-standard",
        cwd=workspace,
    ).splitlines()
    return sorted(set(committed + untracked))


def start_process(
    binary: Path,
    workspace: Path,
    environment: dict[str, str],
    stderr_path: Path,
    transport_retries: int,
) -> subprocess.Popen[bytes]:
    stderr_stream = stderr_path.open("wb")
    try:
        return subprocess.Popen(
            [
                str(binary),
                "app-server",
                "--stdio",
                "--transport-max-retries",
                str(transport_retries),
            ],
            cwd=workspace,
            env=environment,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=stderr_stream,
            start_new_session=True,
        )
    finally:
        stderr_stream.close()


def reopen_projection(
    binary: Path,
    workspace: Path,
    environment: dict[str, str],
    stderr_path: Path,
    resources: dict[str, Any],
    root_id: str,
    child_ids: list[str],
    original_run: dict[str, Any],
    original_events: list[dict[str, Any]],
    original_children: list[list[dict[str, Any]]],
    secret: bytes,
    journal: Journal,
    observation_id: str,
    arm_spec: dict[str, Any],
) -> dict[str, Any]:
    reopen_environment = dict(environment)
    reopen_environment.pop("DEEPSEEK_API_KEY", None)
    process = start_process(
        binary,
        workspace,
        reopen_environment,
        stderr_path,
        resources["transport_max_retries_per_request"],
    )
    client: Any = None
    deadline = time.monotonic() + 30
    try:
        client = M7E.StdioClient(process, secret)
        run_result = client.call(
            M7E.query_envelope("get", root_id, f"m8m-reopen-root-{uuid.uuid4().hex}"),
            M7E.remaining(deadline),
        )
        require(
            run_result.get("kind") == "run"
            and isinstance(run_result.get("run"), dict),
            "reopen_root_missing",
        )
        reopened_events = M7E.fetch_events(
            client, root_id, deadline, f"m8m-reopen-{uuid.uuid4().hex}"
        )
        reopened_children: list[list[dict[str, Any]]] = []
        for index, child_id in enumerate(child_ids):
            child_result = client.call(
                M7E.query_envelope(
                    "get", child_id, f"m8m-reopen-child-{index}-{uuid.uuid4().hex}"
                ),
                M7E.remaining(deadline),
            )
            require(
                child_result.get("kind") == "run"
                and isinstance(child_result.get("run"), dict),
                "reopen_child_missing",
            )
            reopened_children.append(
                M7E.fetch_events(
                    client,
                    child_id,
                    deadline,
                    f"m8m-reopen-child-events-{index}-{uuid.uuid4().hex}",
                )
            )
        projection = {
            "valid": (
                canonical_hash(run_result["run"]) == canonical_hash(original_run)
                and canonical_hash(reopened_events) == canonical_hash(original_events)
                and canonical_hash(reopened_children)
                == canonical_hash(original_children)
            ),
            "root_run_sha256": canonical_hash(run_result["run"]),
            "root_events_sha256": canonical_hash(reopened_events),
            "child_events_sha256": canonical_hash(reopened_children),
        }
        journal.emit(
            {
                "record_type": "sqlite_reopen_snapshot",
                "observation_id": observation_id,
                "arm": arm_spec,
                "credential_present": False,
                "canonical_store_facts": {
                    "run_view": run_result["run"],
                    "root_events": reopened_events,
                    "child_events": reopened_children,
                },
                "projection": projection,
                "key_accessed": True,
                "network_accessed": True,
                "product_metric_eligible": False,
            }
        )
        return projection
    finally:
        if client is not None:
            client.close()
        M7E.stop_process(process)


def execute_arm(
    manifest: dict[str, Any],
    tasks: dict[str, Any],
    arm_spec: dict[str, Any],
    workspace: Path,
    codewhale_source: Path,
    tui_source: Path,
    pair_identity: dict[str, Any],
    revision: str,
    key: str,
    journal: Journal,
) -> dict[str, Any]:
    task_id = arm_spec["task_id"]
    variant = arm_spec["variant"]
    task = tasks["tasks"][task_id]
    observation_id = (
        f"m8m-arm-{arm_spec['schedule_position']:02d}-"
        f"{task_id}-{variant}-{arm_spec['run_index']}"
    )
    started = time.monotonic()
    resources = manifest["resources"]
    deadline = started + resources["harness_wall_time_seconds"]
    baseline_text = (
        ROOT / manifest["prompt_treatment"]["baseline"]["source"]
    ).read_text(encoding="utf-8")
    candidate_text = CANDIDATE_PROMPT_PATH.read_text(encoding="utf-8")
    with tempfile.TemporaryDirectory(
        prefix=f"codewhale-m8m-{task_id}-{variant}-"
    ) as raw:
        root = Path(raw)
        base = materialize_fixture(tasks, task_id, workspace)
        state_root = root / "state"
        home = state_root / "home"
        codewhale_home = state_root / "codewhale"
        xdg = state_root / "xdg"
        for directory in (home, codewhale_home, xdg):
            directory.mkdir(parents=True)
        binary = root / "codewhale"
        tui_binary = root / "codewhale-tui"
        shutil.copy2(codewhale_source, binary)
        shutil.copy2(tui_source, tui_binary)
        binary.chmod(0o700)
        tui_binary.chmod(0o700)
        require(
            file_hash(binary) == pair_identity["codewhale"]["sha256"]
            and file_hash(tui_binary) == pair_identity["codewhale_tui"]["sha256"],
            "binary_pair_changed_after_preflight",
        )
        environment = {
            **M7E.safe_env(),
            "HOME": str(home),
            "CODEWHALE_HOME": str(codewhale_home),
            "XDG_CONFIG_HOME": str(xdg),
            "DEEPSEEK_API_KEY": key,
        }
        if variant == "candidate":
            override = codewhale_home / "prompts/constitution.md"
            override.parent.mkdir(parents=True)
            shutil.copy2(CANDIDATE_PROMPT_PATH, override)
            override.chmod(0o600)
            environment["CODEWHALE_ALLOW_BASE_PROMPT_OVERRIDE"] = "1"
        else:
            require(
                not (codewhale_home / "prompts/constitution.md").exists(),
                "baseline_override_present",
            )
            environment.pop("CODEWHALE_ALLOW_BASE_PROMPT_OVERRIDE", None)
        secret = key.encode("utf-8")
        stderr_path = state_root / "app-server.stderr"
        process = start_process(
            binary,
            workspace,
            environment,
            stderr_path,
            resources["transport_max_retries_per_request"],
        )
        client: Any = None
        root_id = ""
        run: dict[str, Any] = {}
        root_events: list[dict[str, Any]] = []
        children: list[dict[str, Any]] = []
        child_ledgers: list[list[dict[str, Any]]] = []
        try:
            client = M7E.StdioClient(process, secret)
            suffix = (
                f"{task_id}-{variant}-{arm_spec['run_index']}-{uuid.uuid4().hex}"
            )
            command = start_envelope(
                manifest,
                tasks,
                task_id,
                workspace,
                f"m8m-start-{suffix}",
            )
            response = client.call(command, M7E.remaining(deadline))
            require(
                response.get("kind") == "run"
                and isinstance(response.get("run"), dict),
                "start_run_missing",
            )
            root_id = response["run"].get("run_id")
            require(isinstance(root_id, str) and root_id, "root_id_missing")
            run = M7E.wait_terminal(client, process, root_id, deadline, suffix)
            root_events = M7E.fetch_events(client, root_id, deadline, suffix)
            children, child_ledgers = current_child_projection(
                client, root_events, deadline, suffix
            )
        finally:
            if client is not None:
                client.close()
            M7E.stop_process(process)
        terminal_payload = {
            "record_type": "terminal_snapshot",
            "observation_id": observation_id,
            "arm": arm_spec,
            "canonical_store_facts": {
                "run_view": run,
                "root_events": root_events,
                "child_runs": children,
                "child_events": child_ledgers,
            },
            "run_view_sha256": canonical_hash(run),
            "root_events_sha256": canonical_hash(root_events),
            "child_runs_sha256": canonical_hash(children),
            "child_events_sha256": canonical_hash(child_ledgers),
            "key_accessed": True,
            "network_accessed": True,
            "product_metric_eligible": False,
        }
        require(
            secret not in canonical_bytes(terminal_payload),
            "credential_in_terminal_snapshot",
        )
        journal.emit(terminal_payload)
        child_ids = [
            event.get("child_run_id")
            for event in M7E.event_values(root_events, "child_started")
            if isinstance(event.get("child_run_id"), str)
        ]
        reopen = reopen_projection(
            binary,
            workspace,
            environment,
            state_root / "app-server-reopen.stderr",
            resources,
            root_id,
            child_ids,
            run,
            root_events,
            child_ledgers,
            secret,
            journal,
            observation_id,
            arm_spec,
        )
        external = M7E.external_verifier(workspace, deadline)
        verifier_payload = {
            "record_type": "verifier_snapshot",
            "observation_id": observation_id,
            "arm": arm_spec,
            "external_verifier": external,
            "key_accessed": True,
            "network_accessed": True,
            "product_metric_eligible": False,
        }
        require(
            secret not in canonical_bytes(verifier_payload),
            "credential_in_verifier_snapshot",
        )
        journal.emit(verifier_payload)
        changed = observed_changed_files(
            workspace,
            base,
            task["lane"],
        )
        expected_changed = sorted(task["expected_changed_files"])
        child_is_valid = child_valid(manifest, task, children)
        lifecycle = writer_lifecycle(
            manifest, task, root_events, children, workspace
        )
        verification = M7E.verification_projection(
            "t3" if task["evidence_policy"] == "failed_write_pass" else task_id,
            root_events,
        )
        requests = request_projection(
            variant,
            root_events,
            child_ledgers,
            baseline_text,
            candidate_text,
        )
        accounting = accounting_projection(run)
        root_tool_names = [
            event.get("invocation", {}).get("name")
            for event in M7E.event_values(root_events, "tool_prepared")
        ]
        root_allowed = (
            manifest["tool_policy"]["writer_root_tools"]
            if task["lane"] == "explicit_writer"
            else manifest["tool_policy"]["root_tools"]
        )
        tool_authority = all(name in root_allowed for name in root_tool_names)
        terminal = M7E.terminal_projection(run)
        scope_valid = changed == expected_changed and all(
            path in task["allowed_paths"] for path in changed
        )
        behavioral_verified = bool(
            terminal["state"] == "completed"
            and verification["valid"]
            and external["passed"]
            and external["workspace_unchanged"]
            and scope_valid
            and child_is_valid
            and lifecycle["valid"]
            and tool_authority
        )
        measurement_valid = bool(
            accounting["valid"] and requests["valid"] and root_events
        )
        tool = tool_metrics(
            root_events, child_ledgers, behavioral_verified and measurement_valid
        )
        state = M7E.state_schema(codewhale_home)
        measurement_valid = bool(
            measurement_valid and state["valid"] and reopen["valid"]
        )
        result = {
            "task_id": task_id,
            "lane": task["lane"],
            "variant": variant,
            "run_index": arm_spec["run_index"],
            "pair_index": arm_spec["pair_index"],
            "pair_order": arm_spec["pair_order"],
            "revision": revision,
            "binary_pair_sha256": pair_identity["pair_sha256"],
            "fixture_base_commit": base,
            "fixture_tree_sha256": M7E.fixture_hash(tasks, task_id),
            "task_definition_sha256": canonical_hash(
                task_definition(manifest, tasks, task_id)
            ),
            "command_sha256": canonical_hash(
                start_envelope(manifest, tasks, task_id, workspace, "<request-id>")[
                    "command"
                ]
            ),
            "workspace_slot_sha256": sha256_bytes(
                str(workspace.resolve()).encode("utf-8")
            ),
            "terminal": terminal,
            "behavioral_verified": behavioral_verified,
            "measurement_valid": measurement_valid,
            "verified_success": behavioral_verified and measurement_valid,
            "false_success": terminal["state"] == "completed"
            and not behavioral_verified,
            "accounting": accounting,
            "request_identity": requests,
            "verification": verification,
            "tool": tool,
            "child": {
                "valid": child_is_valid,
                "count": len(children),
                "children": children,
            },
            "writer": lifecycle,
            "external_verifier": external,
            "changed_files": changed,
            "scope_valid": scope_valid,
            "tool_authority_valid": tool_authority,
            "state_schema": state,
            "sqlite_reopen": reopen,
            "event_counts": dict(
                sorted(Counter(M7E.event_kind(event) for event in root_events).items())
            ),
            "wall_time_ms": int((time.monotonic() - started) * 1000),
        }
        for path in (
            stderr_path,
            state_root / "app-server-reopen.stderr",
        ):
            require(secret not in path.read_bytes(), "credential_in_stderr")
        require(not M7E.tree_contains(workspace, secret), "credential_in_workspace")
        require(not M7E.tree_contains(state_root, secret), "credential_in_state")
        require(secret not in canonical_bytes(result), "credential_in_result")
        return result


def first_root_non_prompt(fingerprint: dict[str, Any]) -> dict[str, Any]:
    return {
        "actor": fingerprint.get("actor"),
        "model": fingerprint.get("model"),
        "reasoning_effort": fingerprint.get("reasoning_effort"),
        "semantic_messages_sha256": fingerprint.get("semantic_messages_sha256"),
        "task_generation_count": fingerprint.get("task_generation_count"),
        "tools_sha256": fingerprint.get("tools_sha256"),
        "max_output_tokens": fingerprint.get("max_output_tokens"),
        "streaming": fingerprint.get("streaming"),
    }


def paired_identity_matches(
    baseline: dict[str, Any],
    candidate: dict[str, Any],
) -> bool:
    before = baseline.get("request_identity", {}).get("first_root")
    after = candidate.get("request_identity", {}).get("first_root")
    if not isinstance(before, dict) or not isinstance(after, dict):
        return False
    before_prompt = before.get("prompt", {})
    after_prompt = after.get("prompt", {})
    return bool(
        baseline.get("revision") == candidate.get("revision")
        and baseline.get("binary_pair_sha256")
        == candidate.get("binary_pair_sha256")
        and baseline.get("fixture_tree_sha256")
        == candidate.get("fixture_tree_sha256")
        and baseline.get("task_definition_sha256")
        == candidate.get("task_definition_sha256")
        and baseline.get("command_sha256") == candidate.get("command_sha256")
        and baseline.get("workspace_slot_sha256")
        == candidate.get("workspace_slot_sha256")
        and first_root_non_prompt(before) == first_root_non_prompt(after)
        and before_prompt.get("prefix_valid") is True
        and after_prompt.get("prefix_valid") is True
        and before_prompt.get("stable_block_sha256")
        != after_prompt.get("stable_block_sha256")
        and before_prompt.get("stable_suffix_sha256")
        == after_prompt.get("stable_suffix_sha256")
        and before_prompt.get("remaining_blocks_sha256")
        == after_prompt.get("remaining_blocks_sha256")
        and before_prompt.get("block_count") == after_prompt.get("block_count")
        and before_prompt.get("cache_controls")
        == after_prompt.get("cache_controls")
    )


def accounting_abort_code(
    manifest: dict[str, Any],
    arm: dict[str, Any],
) -> str | None:
    accounting = arm.get("accounting", {})
    if accounting.get("billing_unknown") is not False:
        return "aborted_unknown_billing"
    if accounting.get("unpriced") is not False:
        return "aborted_unpriced"
    if accounting.get("sealed") is not True:
        return "aborted_unsealed_accounting"
    if accounting.get("complete") is not True or accounting.get("usage_complete") is not True:
        return "aborted_incomplete_accounting"
    if not arm.get("measurement_valid"):
        return "aborted_measurement_invalid"
    if (
        accounting.get("cost_nanousd", 0)
        > manifest["resources"]["max_known_cost_nanousd_per_arm"]
    ):
        return "aborted_per_arm_cost_threshold"
    if arm.get("writer", {}).get("required") and not arm["writer"].get("valid"):
        return "aborted_writer_lifecycle_invalid"
    return None


def suite_abort_code(
    manifest: dict[str, Any],
    arms: list[dict[str, Any]],
) -> str | None:
    latest = arms[-1]
    arm_abort = accounting_abort_code(manifest, latest)
    if arm_abort is not None:
        return arm_abort
    pair = [
        arm
        for arm in arms
        if arm["task_id"] == latest["task_id"]
        and arm["run_index"] == latest["run_index"]
    ]
    if len(pair) < 2:
        return None
    if len(pair) != 2 or {arm["variant"] for arm in pair} != set(VARIANTS):
        return "aborted_pair_shape_invalid"
    by_variant = {arm["variant"]: arm for arm in pair}
    if not paired_identity_matches(
        by_variant["baseline"], by_variant["candidate"]
    ):
        return "aborted_paired_treatment_identity_mismatch"
    return None


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


def median_fraction(values: list[Fraction]) -> Fraction | None:
    if not values:
        return None
    ordered = sorted(values)
    middle = len(ordered) // 2
    if len(ordered) % 2:
        return ordered[middle]
    return (ordered[middle - 1] + ordered[middle]) / 2


def paired_summary(arms: list[dict[str, Any]]) -> dict[str, Any]:
    pairs: dict[tuple[str, int], dict[str, dict[str, Any]]] = defaultdict(dict)
    for arm in arms:
        pairs[(arm["task_id"], arm["run_index"])][arm["variant"]] = arm
    complete = [pair for pair in pairs.values() if set(pair) == set(VARIANTS)]
    dual = [
        pair
        for pair in complete
        if all(pair[variant]["verified_success"] for variant in VARIANTS)
    ]
    metrics: dict[str, Any] = {}
    for name in ("physical_requests", "tokens", "cost_nanousd", "wall_time_ms"):
        improvements: list[Fraction] = []
        for pair in dual:
            before = arm_metric(pair["baseline"], name)
            after = arm_metric(pair["candidate"], name)
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
        "complete_pairs": len(complete),
        "dual_success_pairs": len(dual),
        "identity_valid_pairs": sum(
            paired_identity_matches(pair["baseline"], pair["candidate"])
            for pair in complete
        ),
        "metrics": metrics,
    }


def decide(
    manifest: dict[str, Any],
    arms: list[dict[str, Any]],
) -> dict[str, Any]:
    if len(arms) != manifest["experiment"]["formal_arms"]:
        return {"decision": "hold", "reason": "formal_cells_incomplete"}
    cells = Counter((arm["task_id"], arm["variant"]) for arm in arms)
    if set(cells.values()) != {manifest["experiment"]["runs_per_variant_task"]}:
        return {"decision": "hold", "reason": "formal_cell_shape_invalid"}
    if any(arm["false_success"] for arm in arms):
        return {"decision": "reject", "reason": "false_success"}
    if any(
        not arm["measurement_valid"]
        or not arm["scope_valid"]
        or not arm["tool_authority_valid"]
        or not arm["child"]["valid"]
        or not arm["writer"]["valid"]
        or not arm["sqlite_reopen"]["valid"]
        for arm in arms
    ):
        return {"decision": "reject", "reason": "hard_gate_failed"}
    by_pair: dict[tuple[str, int], dict[str, dict[str, Any]]] = defaultdict(dict)
    for arm in arms:
        by_pair[(arm["task_id"], arm["run_index"])][arm["variant"]] = arm
    if any(
        set(pair) != set(VARIANTS)
        or not paired_identity_matches(pair["baseline"], pair["candidate"])
        for pair in by_pair.values()
    ):
        return {"decision": "reject", "reason": "paired_identity_invalid"}
    for task_id in {arm["task_id"] for arm in arms}:
        before = sum(
            arm["verified_success"]
            for arm in arms
            if arm["task_id"] == task_id and arm["variant"] == "baseline"
        )
        after = sum(
            arm["verified_success"]
            for arm in arms
            if arm["task_id"] == task_id and arm["variant"] == "candidate"
        )
        if after < before:
            return {
                "decision": "reject",
                "reason": "verified_success_regression",
                "task_id": task_id,
            }
    before_first = sum(
        arm["tool"]["first_tool_correct"]
        for arm in arms
        if arm["variant"] == "baseline"
    )
    after_first = sum(
        arm["tool"]["first_tool_correct"]
        for arm in arms
        if arm["variant"] == "candidate"
    )
    before_recovery = sum(
        arm["tool"]["recovered_after_failure"]
        for arm in arms
        if arm["variant"] == "baseline"
    )
    after_recovery = sum(
        arm["tool"]["recovered_after_failure"]
        for arm in arms
        if arm["variant"] == "candidate"
    )
    if after_first < before_first or after_recovery < before_recovery:
        return {"decision": "reject", "reason": "tool_or_recovery_regression"}
    paired = paired_summary(arms)
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
    benefit = any(value >= 10 for value in improvements)
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


def aggregate(
    manifest: dict[str, Any],
    arms: list[dict[str, Any]],
) -> dict[str, Any]:
    cells: dict[str, Any] = {}
    for task_id in [task["id"] for task in manifest["task_sources"]["tasks"]]:
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
                "first_tool_correct": sum(
                    arm["tool"]["first_tool_correct"] for arm in selected
                ),
                "first_edit_success": sum(
                    arm["tool"]["first_edit_success"] for arm in selected
                ),
                "recovered_after_failure": sum(
                    arm["tool"]["recovered_after_failure"] for arm in selected
                ),
                "physical_requests": sum(
                    arm["accounting"]["requests"]["started"] for arm in selected
                ),
                "input_tokens": sum(
                    arm["accounting"]["usage"]["input_tokens"] for arm in selected
                ),
                "output_tokens": sum(
                    arm["accounting"]["usage"]["output_tokens"] for arm in selected
                ),
                "cache_hit_tokens": sum(
                    arm["accounting"]["usage"]["cache_hit_tokens"] for arm in selected
                ),
                "cache_miss_tokens": sum(
                    arm["accounting"]["usage"]["cache_miss_tokens"] for arm in selected
                ),
                "cost_nanousd": sum(
                    arm["accounting"]["cost_nanousd"] for arm in selected
                ),
                "wall_time_ms": sum(arm["wall_time_ms"] for arm in selected),
            }
    return {
        "arms": len(arms),
        "cells": cells,
        "paired": paired_summary(arms),
        "decision": decide(manifest, arms),
    }


def probe_binary(path: Path, revision: str) -> dict[str, Any]:
    require(path.is_file() and not path.is_symlink(), "binary_invalid")
    result = M7E.run_command([str(path), "--version"], cwd=ROOT, timeout=30)
    require(result.returncode == 0, "binary_version_failed")
    version = (result.stdout + result.stderr).decode("utf-8", errors="strict").strip()
    require(revision[:12] in version, "binary_revision_mismatch")
    return {
        "version": version,
        "sha256": file_hash(path),
        "size_bytes": path.stat().st_size,
    }


def activation_identity_matches(
    baseline: dict[str, Any],
    candidate: dict[str, Any],
) -> bool:
    before_prompt = baseline.get("prompt", {})
    after_prompt = candidate.get("prompt", {})
    return bool(
        baseline.get("non_prompt") == candidate.get("non_prompt")
        and before_prompt.get("prefix_valid") is True
        and after_prompt.get("prefix_valid") is True
        and before_prompt.get("stable_block_sha256")
        != after_prompt.get("stable_block_sha256")
        and before_prompt.get("stable_suffix_sha256")
        == after_prompt.get("stable_suffix_sha256")
        and before_prompt.get("remaining_blocks_sha256")
        == after_prompt.get("remaining_blocks_sha256")
        and before_prompt.get("block_count") == after_prompt.get("block_count")
        and before_prompt.get("cache_controls")
        == after_prompt.get("cache_controls")
    )


def probe_process_prompt_activation(
    binary: Path,
    manifest: dict[str, Any],
    tasks: dict[str, Any],
) -> dict[str, Any]:
    baseline_text = (
        ROOT / manifest["prompt_treatment"]["baseline"]["source"]
    ).read_text(encoding="utf-8")
    candidate_text = CANDIDATE_PROMPT_PATH.read_text(encoding="utf-8")
    with tempfile.TemporaryDirectory(prefix="codewhale-m8m-activation-") as raw:
        root = Path(raw)
        workspace = root / "workspace"
        materialize_fixture(tasks, "t1", workspace)
        variants: dict[str, dict[str, Any]] = {}
        for variant in VARIANTS:
            state_root = root / variant
            home = state_root / "home"
            codewhale_home = state_root / "codewhale"
            xdg = state_root / "xdg"
            for directory in (home, codewhale_home, xdg):
                directory.mkdir(parents=True)
            environment = {
                **M7E.safe_env(),
                "HOME": str(home),
                "CODEWHALE_HOME": str(codewhale_home),
                "XDG_CONFIG_HOME": str(xdg),
                "DEEPSEEK_API_KEY": "offline-m8m-activation-key",
                "HTTPS_PROXY": "http://127.0.0.1:1",
                "HTTP_PROXY": "http://127.0.0.1:1",
                "ALL_PROXY": "http://127.0.0.1:1",
                "https_proxy": "http://127.0.0.1:1",
                "http_proxy": "http://127.0.0.1:1",
                "all_proxy": "http://127.0.0.1:1",
                "NO_PROXY": "",
                "no_proxy": "",
            }
            if variant == "candidate":
                override = codewhale_home / "prompts/constitution.md"
                override.parent.mkdir(parents=True)
                shutil.copy2(CANDIDATE_PROMPT_PATH, override)
                override.chmod(0o600)
                environment["CODEWHALE_ALLOW_BASE_PROMPT_OVERRIDE"] = "1"
            process = start_process(
                binary,
                workspace,
                environment,
                state_root / "app-server.stderr",
                0,
            )
            client: Any = None
            run_id = ""
            request: dict[str, Any] = {}
            try:
                client = M7E.StdioClient(process, b"offline-m8m-activation-key")
                envelope = start_envelope(
                    manifest,
                    tasks,
                    "t1",
                    workspace,
                    f"m8m-v2-activation-{variant}",
                )
                envelope["command"]["max_api_requests"] = 1
                envelope["command"]["limits"].update(
                    {
                        "max_turns": 1,
                        "max_model_requests": 1,
                        "max_model_retries": 0,
                        "max_tool_calls": 0,
                        "wall_time_ms": 15000,
                    }
                )
                result = client.call(envelope, 15)
                require(
                    result.get("kind") == "run"
                    and isinstance(result.get("run"), dict)
                    and isinstance(result["run"].get("run_id"), str),
                    "activation_run_missing",
                )
                run_id = result["run"]["run_id"]
                events = M7E.fetch_events(
                    client,
                    run_id,
                    time.monotonic() + 15,
                    f"m8m-v2-activation-{variant}",
                )
                created = M7E.event_values(events, "run_created")
                require(
                    len(created) == 1 and isinstance(created[0].get("request"), dict),
                    "activation_run_created_missing",
                )
                request = created[0]["request"]
                task_contract = request.get("task_contract", {})
                variants[variant] = {
                    "prompt": prompt_signature(
                        request, variant, baseline_text, candidate_text
                    ),
                    "non_prompt": {
                        "model": request.get("model"),
                        "reasoning_effort": request.get("reasoning_effort"),
                        "max_output_tokens": request.get("max_output_tokens"),
                        "streaming": request.get("streaming"),
                        "actor": request.get("actor"),
                        "tool_policy_sha256": canonical_hash(
                            request.get("tool_policy")
                        ),
                        "limits_sha256": canonical_hash(request.get("limits")),
                        "context_policy_sha256": canonical_hash(
                            request.get("context_policy")
                        ),
                        "task_definition_sha256": canonical_hash(
                            task_contract.get("definition")
                        ),
                        "workspace": request.get("environment", {}).get(
                            "workspace"
                        ),
                        "tool_catalog_sha256": request.get("environment", {}).get(
                            "tool_catalog_sha256"
                        ),
                        "execution_fingerprint_sha256": request.get(
                            "environment", {}
                        ).get("execution_fingerprint_sha256"),
                    },
                }
            finally:
                if client is not None:
                    client.close()
                M7E.stop_process(process)
            reopen_environment = dict(environment)
            reopen_environment.pop("DEEPSEEK_API_KEY", None)
            reopen_process = start_process(
                binary,
                workspace,
                reopen_environment,
                state_root / "app-server-reopen.stderr",
                0,
            )
            reopen_client: Any = None
            try:
                reopen_client = M7E.StdioClient(
                    reopen_process,
                    b"offline-m8m-activation-key",
                )
                reopened = reopen_client.call(
                    M7E.query_envelope(
                        "get",
                        run_id,
                        f"m8m-activation-reopen-{variant}",
                    ),
                    15,
                )
                reopened_events = M7E.fetch_events(
                    reopen_client,
                    run_id,
                    time.monotonic() + 15,
                    f"m8m-activation-reopen-events-{variant}",
                )
                reopened_created = M7E.event_values(
                    reopened_events,
                    "run_created",
                )
                require(
                    reopened.get("kind") == "run"
                    and len(reopened_created) == 1
                    and canonical_hash(reopened_created[0].get("request"))
                    == canonical_hash(request),
                    "activation_no_credential_reopen_invalid",
                )
                variants[variant]["no_credential_reopen"] = True
            finally:
                if reopen_client is not None:
                    reopen_client.close()
                M7E.stop_process(reopen_process)
        require(
            activation_identity_matches(
                variants["baseline"], variants["candidate"]
            ),
            "process_prompt_activation_invalid",
        )
        return {
            "network": "blocked_by_loopback_proxy",
            "external_api_requests": 0,
            "same_non_prompt_identity": True,
            "no_credential_reopen": all(
                value.get("no_credential_reopen") is True
                for value in variants.values()
            ),
            "variants": variants,
        }


def preflight_identity(
    manifest: dict[str, Any],
    tasks: dict[str, Any],
    codewhale: Path,
    tui: Path,
    revision: str,
) -> dict[str, Any]:
    source = manifest["source_identity"]
    require(len(revision) == 40, "revision_invalid")
    require(
        source.get("candidate_binary_revision") == revision,
        "candidate_revision_not_frozen",
    )
    require(M7E.git_output("status", "--short") == "", "worktree_not_clean")
    require(
        isinstance(source.get("harness_revision"), str)
        and len(source["harness_revision"]) == 40
        and M7E.git_output("cat-file", "-t", source["harness_revision"]) == "commit",
        "harness_revision_missing",
    )
    require(
        M7E.git_output("cat-file", "-t", revision) == "commit",
        "candidate_revision_missing",
    )
    candidate_at_revision = M7E.run_command(
        ["git", "show", f"{revision}:eval/fixtures/m8-d-prompt/v1/constitution.md"],
        cwd=ROOT,
    )
    require(
        candidate_at_revision.returncode == 0
        and sha256_bytes(candidate_at_revision.stdout)
        == manifest["prompt_treatment"]["candidate"]["sha256"],
        "candidate_not_bound_to_binary_revision",
    )
    for path in (
        Path(__file__).resolve(),
        TEST_PATH,
        M7E_PATH,
    ):
        relative = path.relative_to(ROOT).as_posix()
        frozen_file = M7E.run_command(
            ["git", "show", f"{source['harness_revision']}:{relative}"],
            cwd=ROOT,
        )
        require(
            frozen_file.returncode == 0
            and sha256_bytes(frozen_file.stdout) == file_hash(path),
            "harness_revision_file_mismatch",
            {"path": relative},
        )
    main_identity = probe_binary(codewhale, revision)
    tui_identity = probe_binary(tui, revision)
    pair = {
        "codewhale": main_identity,
        "codewhale_tui": tui_identity,
    }
    pair["pair_sha256"] = canonical_hash(pair)
    frozen_binary = manifest["binary_identity"]
    require(
        main_identity == frozen_binary["codewhale"]
        and tui_identity == frozen_binary["codewhale_tui"]
        and pair["pair_sha256"] == frozen_binary["pair_sha256"],
        "binary_pair_identity_mismatch",
    )
    source_tree = M7E.git_output("rev-parse", f"{revision}^{{tree}}")
    require(
        source_tree == source.get("candidate_source_tree"),
        "candidate_source_tree_mismatch",
    )
    activation = probe_process_prompt_activation(codewhale, manifest, tasks)
    fixture_identities = probe_fixture_identities(tasks)
    return {
        "revision": revision,
        "source_tree": source_tree,
        "harness_revision": source["harness_revision"],
        "binary_pair": pair,
        "manifest_sha256": file_hash(MANIFEST_PATH),
        "manifest_content_sha256": manifest_content_hash(manifest),
        "harness_sha256": file_hash(Path(__file__).resolve()),
        "harness_test_sha256": file_hash(TEST_PATH),
        "m7e_projection_sha256": file_hash(M7E_PATH),
        "candidate_prompt_sha256": file_hash(CANDIDATE_PROMPT_PATH),
        "schedule_sha256": canonical_hash(formal_schedule(manifest)),
        "schedule_arms": len(formal_schedule(manifest)),
        "task_definition_sha256": {
            task_id: canonical_hash(task_definition(manifest, tasks, task_id))
            for task_id in tasks["tasks"]
        },
        "process_prompt_activation": activation,
        "fixture_identities": fixture_identities,
    }


def pair_workspace_slot(
    root: Path,
    task_id: str,
    run_index: int,
) -> Path:
    workspace = root / f"{task_id}-run-{run_index}"
    resolved_root = root.resolve()
    resolved_parent = workspace.parent.resolve()
    require(resolved_parent == resolved_root, "workspace_slot_escape")
    return workspace


def clear_workspace(workspace: Path, root: Path) -> None:
    require(workspace.parent.resolve() == root.resolve(), "workspace_cleanup_escape")
    if workspace.exists():
        shutil.rmtree(workspace)


def validate_output_path(path: Path) -> None:
    require(path.suffix == ".jsonl", "output_suffix_invalid")
    require(
        path.parent.resolve() == (ROOT / "eval/raw").resolve(),
        "output_directory_invalid",
    )
    require(not path.exists(), "output_exists")


def run_formal(args: argparse.Namespace) -> int:
    manifest, tasks = load_manifest(frozen=True)
    require(
        manifest.get("admission", {}).get("live_api_admitted") is True,
        "live_api_not_admitted",
    )
    require(args.acknowledge_cost, "cost_not_acknowledged")
    identity = preflight_identity(
        manifest,
        tasks,
        Path(args.binary).resolve(),
        Path(args.tui_binary).resolve(),
        args.revision,
    )
    schedule = formal_schedule(manifest)
    output = Path(args.output).resolve()
    validate_output_path(output)
    key = ""
    active_arm: dict[str, Any] | None = None
    arms: list[dict[str, Any]] = []
    with Journal.claim(output) as journal:
        journal.emit(
            {
                "record_type": "suite_plan",
                "result_schema": RESULT_SCHEMA,
                "created_at_unix_ms": int(time.time() * 1000),
                "identity": identity,
                "schedule": schedule,
                "maximum_reruns": 0,
                "key_accessed": False,
                "network_accessed": False,
                "product_metric_eligible": False,
            }
        )
        try:
            key = M7E.read_key(Path(args.key_file).resolve())
        except M7E.EvaluationError as error:
            journal.emit(
                {
                    "record_type": "suite_abort",
                    "code": error.code,
                    "details": error.details,
                    "active_arm": None,
                    "maximum_reruns": 0,
                    "key_accessed": False,
                    "network_accessed": False,
                    "product_metric_eligible": False,
                }
            )
            return 2
        secret = key.encode("utf-8")
        journal.emit(
            {
                "record_type": "credential_access",
                "key_accessed": True,
                "network_accessed": False,
                "product_metric_eligible": False,
            }
        )
        try:
            require(secret not in output.read_bytes(), "credential_in_reservation")
            with tempfile.TemporaryDirectory(prefix="codewhale-m8m-suite-") as raw:
                suite_root = Path(raw)
                binaries = suite_root / "bin"
                workspace_root = suite_root / "paired-workspaces"
                binaries.mkdir()
                workspace_root.mkdir()
                codewhale = binaries / "codewhale"
                tui = binaries / "codewhale-tui"
                shutil.copy2(Path(args.binary).resolve(), codewhale)
                shutil.copy2(Path(args.tui_binary).resolve(), tui)
                codewhale.chmod(0o700)
                tui.chmod(0o700)
                require(
                    file_hash(codewhale)
                    == identity["binary_pair"]["codewhale"]["sha256"]
                    and file_hash(tui)
                    == identity["binary_pair"]["codewhale_tui"]["sha256"],
                    "suite_binary_pair_changed",
                )
                for arm_spec in schedule:
                    known_cost = sum(
                        arm["accounting"]["cost_nanousd"] for arm in arms
                    )
                    if (
                        known_cost
                        + manifest["resources"]["max_known_cost_nanousd_per_arm"]
                        > manifest["resources"]["formal_suite_known_cost_nanousd"]
                    ):
                        raise EvaluationError(
                            "suite_known_cost_headroom_exhausted"
                        )
                    active_arm = arm_spec
                    journal.emit(
                        {
                            "record_type": "arm_plan",
                            "arm": arm_spec,
                            "known_cost_nanousd_before_arm": known_cost,
                            "maximum_reruns": 0,
                            "key_accessed": True,
                            "network_accessed": False,
                            "product_metric_eligible": False,
                        }
                    )
                    workspace = pair_workspace_slot(
                        workspace_root,
                        arm_spec["task_id"],
                        arm_spec["run_index"],
                    )
                    clear_workspace(workspace, workspace_root)
                    try:
                        arm = execute_arm(
                            manifest,
                            tasks,
                            arm_spec,
                            workspace,
                            codewhale,
                            tui,
                            identity["binary_pair"],
                            args.revision,
                            key,
                            journal,
                        )
                    finally:
                        clear_workspace(workspace, workspace_root)
                    require(secret not in canonical_bytes(arm), "credential_in_arm")
                    observed = [*arms, arm]
                    abort_code = suite_abort_code(manifest, observed)
                    journal.emit(
                        {
                            "record_type": "arm_observation",
                            "arm": arm_spec,
                            "projection": arm,
                            "abort_code": abort_code,
                            "maximum_reruns": 0,
                            "key_accessed": True,
                            "network_accessed": True,
                            "product_metric_eligible": False,
                        }
                    )
                    if abort_code is not None:
                        raise EvaluationError(abort_code)
                    arms = observed
                    active_arm = None
        except EVALUATION_ERRORS as error:
            journal.emit(
                {
                    "record_type": "suite_abort",
                    "code": error.code,
                    "details": error.details,
                    "active_arm": active_arm,
                    "completed_measurement_valid_arms": len(arms),
                    "known_cost_nanousd_lower_bound": sum(
                        arm["accounting"]["cost_nanousd"] for arm in arms
                    ),
                    "maximum_reruns": 0,
                    "key_accessed": True,
                    "network_accessed": True,
                    "product_metric_eligible": False,
                }
            )
            return 2
        except (OSError, UnicodeError, subprocess.SubprocessError) as error:
            journal.emit(
                {
                    "record_type": "suite_abort",
                    "code": "harness_internal_error",
                    "error_type": type(error).__name__,
                    "active_arm": active_arm,
                    "completed_measurement_valid_arms": len(arms),
                    "known_cost_nanousd_lower_bound": sum(
                        arm["accounting"]["cost_nanousd"] for arm in arms
                    ),
                    "maximum_reruns": 0,
                    "key_accessed": True,
                    "network_accessed": True,
                    "product_metric_eligible": False,
                }
            )
            return 2
        result = aggregate(manifest, arms)
        journal.emit(
            {
                "record_type": "suite_result",
                "completed_arms": len(arms),
                "aggregate": result,
                "maximum_reruns": 0,
                "key_accessed": True,
                "network_accessed": True,
                "product_metric_eligible": True,
            }
        )
    secret = key.encode("utf-8")
    require(secret not in output.read_bytes(), "credential_in_output")
    audit = read_journal(output, allow_partial_tail=False)
    require(
        audit["partial_tail_bytes"] == 0
        and audit["records"][-1]["payload"].get("record_type")
        == "suite_result",
        "formal_journal_incomplete",
    )
    print(
        json.dumps(
            {
                "status": "complete",
                "output": str(output),
                "arms": len(arms),
                "decision": result["decision"],
                "journal_sha256": audit["file_sha256"],
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
        "m7e_projection_sha256": file_hash(M7E_PATH),
        "candidate_prompt_sha256": file_hash(CANDIDATE_PROMPT_PATH),
        "schedule_sha256": canonical_hash(formal_schedule(manifest)),
        "schedule_arms": len(formal_schedule(manifest)),
        "journal_fault_matrix_sha256": journal_fault_contract_hash(),
        "fixtures": {
            task_id: M7E.fixture_hash(tasks, task_id)
            for task_id in tasks["tasks"]
        },
        "task_definitions": {
            task_id: canonical_hash(task_definition(manifest, tasks, task_id))
            for task_id in tasks["tasks"]
        },
    }


def preflight_command(args: argparse.Namespace) -> int:
    manifest, tasks = load_manifest(frozen=True)
    identity = preflight_identity(
        manifest,
        tasks,
        Path(args.binary).resolve(),
        Path(args.tui_binary).resolve(),
        args.revision,
    )
    print(json.dumps(identity, ensure_ascii=False, sort_keys=True, indent=2))
    return 0


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    subparsers.add_parser("freeze-report")
    preflight = subparsers.add_parser("preflight")
    preflight.add_argument("--binary", required=True)
    preflight.add_argument("--tui-binary", required=True)
    preflight.add_argument("--revision", required=True)
    formal = subparsers.add_parser("formal")
    formal.add_argument("--binary", required=True)
    formal.add_argument("--tui-binary", required=True)
    formal.add_argument("--revision", required=True)
    formal.add_argument("--key-file", required=True)
    formal.add_argument("--output", required=True)
    formal.add_argument("--acknowledge-cost", action="store_true")
    subparsers.add_parser("journal-self-test")
    fault = subparsers.add_parser("journal-fault-child")
    fault.add_argument("--fault", choices=JOURNAL_FAULTS, required=True)
    fault.add_argument("--output", required=True)
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
        if args.command == "journal-self-test":
            results = run_journal_fault_matrix()
            print(
                json.dumps(
                    {
                        "status": "passed",
                        "faults": len(results),
                        "contract_sha256": journal_fault_contract_hash(),
                        "results": results,
                    },
                    ensure_ascii=False,
                    sort_keys=True,
                    indent=2,
                )
            )
            return 0
        if args.command == "journal-fault-child":
            return run_journal_fault_child(
                args.fault,
                Path(args.output).resolve(),
            )
        raise AssertionError(args.command)
    except (EvaluationError, M7E.EvaluationError) as error:
        print(
            json.dumps(
                {
                    "status": "error",
                    "code": error.code,
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
