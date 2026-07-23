#!/usr/bin/env python3
"""M7-G2 fail-before-loss observer durability gate.

This evaluator is deliberately offline.  It proves that an evaluation journal
persists exact canonical Run API facts before any identity, verifier, surface,
accounting, or product-metric derivation.  It cannot read a credential, start
CodeWhale, contact DeepSeek, or execute a paid successor suite.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import signal
import stat
import subprocess
import sys
import tempfile
from typing import Any, BinaryIO


ROOT = Path(__file__).resolve().parents[1]
MANIFEST_PATH = ROOT / "eval/manifests/m7-g2-observer-durability-v1.json"
LEGACY_RAW_PATH = (
    ROOT / "eval/results/m7-g-readonly-fanout-062623e6.jsonl"
)
MANIFEST_SCHEMA = "codewhale.eval.m7-g2-observer-durability.v1"
JOURNAL_SCHEMA = "codewhale.eval.m7-g2-observer-journal.v1"
LEGACY_RAW_SHA256 = (
    "sha256:177bb20aa9b1d5d91b75a60ce3fde7c783c0e927567f9c2c79e2ad971fa546eb"
)
ZERO_HASH = "sha256:" + ("0" * 64)
FAULTS = (
    "identity_exception",
    "verifier_exception",
    "surface_exception",
    "accounting_exception",
    "terminal_before_write_kill",
    "terminal_mid_write_kill",
    "terminal_after_write_before_fsync_kill",
    "after_terminal_fsync_kill",
    "after_reopen_fsync_kill",
    "after_verifier_fsync_kill",
    "before_result_kill",
)
SAFE_ENV_NAMES = (
    "PATH",
    "TMPDIR",
    "TMP",
    "TEMP",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
)


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


def safe_env() -> dict[str, str]:
    environment = {
        name: value
        for name in SAFE_ENV_NAMES
        if (value := os.environ.get(name))
    }
    environment.setdefault("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
    environment["PYTHONDONTWRITEBYTECODE"] = "1"
    return environment


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


def record_core(
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


def encode_record(
    sequence: int,
    previous_record_sha256: str,
    payload: dict[str, Any],
) -> tuple[bytes, str]:
    core = record_core(sequence, previous_record_sha256, payload)
    record_sha256 = canonical_hash(core)
    return (
        canonical_bytes({**core, "record_sha256": record_sha256}) + b"\n",
        record_sha256,
    )


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
        require(
            payload.get("key_accessed") is False
            and payload.get("network_accessed") is False,
            "offline_record_identity_invalid",
        )
        encoded, record_sha256 = encode_record(
            self.sequence + 1,
            self.previous_record_sha256,
            payload,
        )
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


def read_journal(
    path: Path,
    *,
    allow_partial_tail: bool,
) -> dict[str, Any]:
    metadata = path.lstat()
    require(
        stat.S_ISREG(metadata.st_mode)
        and not path.is_symlink()
        and stat.S_IMODE(metadata.st_mode) == 0o600,
        "journal_file_invalid",
    )
    raw = path.read_bytes()
    complete = raw.endswith(b"\n")
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
        require(
            isinstance(record, dict)
            and record.get("schema") == JOURNAL_SCHEMA
            and record.get("sequence") == index
            and record.get("previous_record_sha256") == previous
            and isinstance(record.get("payload"), dict),
            "journal_chain_invalid",
        )
        claimed_hash = record.get("record_sha256")
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
            claimed_hash == canonical_hash(core),
            "journal_hash_invalid",
        )
        previous = claimed_hash
        records.append(record)
    return {
        "records": records,
        "complete": complete,
        "partial_tail_bytes": len(tail),
        "partial_tail_sha256": sha256_bytes(tail) if tail else None,
        "file_sha256": sha256_bytes(raw),
    }


def sample_store_facts() -> dict[str, Any]:
    request = {
        "actor": {"kind": "root"},
        "max_output_tokens": 4096,
        "messages": [
            {"role": "system", "content": "stable"},
            {"role": "user", "content": "fixture task"},
        ],
        "model": "deepseek-v4-flash",
        "reasoning_effort": "high",
        "streaming": True,
        "tools": [],
    }
    accounting = {
        "billing_unknown": False,
        "child": {"completed": 0, "in_flight": 0, "started": 0},
        "complete": True,
        "cost_nanocny": 128,
        "cost_nanousd": 18,
        "hard_request_limit": 12,
        "root": {"completed": 1, "in_flight": 0, "started": 1},
        "surface_usage": [
            {
                "model": "deepseek-v4-flash",
                "surface": "standard_chat",
                "usage_response_count": 1,
            }
        ],
        "transport_retries": 0,
        "unpriced": False,
        "usage_complete": True,
        "usage_incomplete": False,
        "usage_missing": False,
    }
    usage = {
        "cache_hit_tokens": 80,
        "cache_miss_tokens": 20,
        "input_tokens": 100,
        "output_tokens": 10,
        "reasoning_tokens": 4,
    }
    terminal = {
        "state": "completed",
        "answer": "fixture",
        "accounting": accounting,
    }
    events = [
        {
            "sequence": 1,
            "schema_version": 16,
            "event": {
                "kind": "run_created",
                "request": {
                    "model": "deepseek-v4-flash",
                    "max_output_tokens": 4096,
                    "reasoning_effort": "high",
                },
            },
        },
        {
            "sequence": 2,
            "schema_version": 16,
            "event": {
                "kind": "model_request_prepared",
                "request": request,
            },
        },
        {
            "sequence": 3,
            "schema_version": 16,
            "event": {
                "kind": "terminal",
                "outcome": terminal,
            },
        },
    ]
    run_view = {
        "run_id": "run-m7g2-offline-fixture",
        "terminal": terminal,
        "usage": usage,
        "accounting": accounting,
        "tool_calls": 0,
    }
    return {
        "run_view": run_view,
        "events": events,
        "state": {
            "schema_version": 21,
            "quick_check": "ok",
            "foreign_key_violations": 0,
        },
    }


def terminal_snapshot_payload() -> dict[str, Any]:
    facts = sample_store_facts()
    return {
        "record_type": "terminal_snapshot",
        "observation_id": "offline-observation-1",
        "canonical_store_facts": facts,
        "run_view_sha256": canonical_hash(facts["run_view"]),
        "events_sha256": canonical_hash(facts["events"]),
        "model_requests": [
            event["event"]["request"]
            for event in facts["events"]
            if event["event"]["kind"] == "model_request_prepared"
        ],
        "key_accessed": False,
        "network_accessed": False,
    }


def reopen_snapshot_payload() -> dict[str, Any]:
    facts = sample_store_facts()
    return {
        "record_type": "sqlite_reopen_snapshot",
        "observation_id": "offline-observation-1",
        "canonical_store_facts": facts,
        "run_view_sha256": canonical_hash(facts["run_view"]),
        "events_sha256": canonical_hash(facts["events"]),
        "matches_terminal_snapshot": True,
        "reopened_without_credential": True,
        "key_accessed": False,
        "network_accessed": False,
    }


def verifier_snapshot_payload(*, execution_error: bool = False) -> dict[str, Any]:
    return {
        "record_type": "verifier_snapshot",
        "observation_id": "offline-observation-1",
        "verifier": {
            "status": "operation_error" if execution_error else "passed",
            "returncode": None if execution_error else 0,
            "stdout_sha256": sha256_bytes(b""),
            "stderr_sha256": sha256_bytes(b""),
        },
        "changed_files": ["fixture.py"],
        "key_accessed": False,
        "network_accessed": False,
    }


def abort_payload(code: str) -> dict[str, Any]:
    return {
        "record_type": "abort",
        "observation_id": "offline-observation-1",
        "error_code": code,
        "product_metric_eligible": False,
        "maximum_reruns": 0,
        "key_accessed": False,
        "network_accessed": False,
    }


def kill_self() -> None:
    os.kill(os.getpid(), signal.SIGKILL)


def run_fault_child(fault: str, output: Path) -> int:
    require(fault in FAULTS, "fault_unknown")
    with Journal.claim(output) as journal:
        journal.emit(
            {
                "record_type": "plan",
                "fault": fault,
                "product_metric_eligible": False,
                "key_accessed": False,
                "network_accessed": False,
            }
        )
        if fault == "terminal_before_write_kill":
            kill_self()
        terminal_fault = {
            "terminal_mid_write_kill": "mid_write_kill",
            "terminal_after_write_before_fsync_kill": (
                "after_write_before_fsync_kill"
            ),
        }.get(fault)
        journal.emit(terminal_snapshot_payload(), fault=terminal_fault)
        if fault == "after_terminal_fsync_kill":
            kill_self()
        journal.emit(reopen_snapshot_payload())
        if fault == "after_reopen_fsync_kill":
            kill_self()
        if fault == "verifier_exception":
            journal.emit(verifier_snapshot_payload(execution_error=True))
            journal.emit(abort_payload("verifier_observer_failed"))
            return 2
        journal.emit(verifier_snapshot_payload())
        if fault == "after_verifier_fsync_kill":
            kill_self()
        if fault == "identity_exception":
            journal.emit(abort_payload("run_identity_invalid"))
            return 2
        if fault == "surface_exception":
            journal.emit(abort_payload("surface_identity_invalid"))
            return 2
        if fault == "accounting_exception":
            journal.emit(abort_payload("accounting_incomplete"))
            return 2
        if fault == "before_result_kill":
            kill_self()
        journal.emit(
            {
                "record_type": "arm_result",
                "observation_id": "offline-observation-1",
                "product_metric_eligible": False,
                "key_accessed": False,
                "network_accessed": False,
            }
        )
    return 0


def payload_types(audit: dict[str, Any]) -> list[str]:
    return [
        record["payload"].get("record_type")
        for record in audit["records"]
    ]


def validate_completed_observation_order(audit: dict[str, Any]) -> None:
    types = payload_types(audit)
    expected = [
        "plan",
        "terminal_snapshot",
        "sqlite_reopen_snapshot",
        "verifier_snapshot",
        "arm_result",
    ]
    require(
        types == expected
        and audit["complete"]
        and audit["partial_tail_bytes"] == 0,
        "completed_observation_order_invalid",
        {"record_types": types},
    )
    terminal = audit["records"][1]["payload"]
    reopen = audit["records"][2]["payload"]
    require(
        terminal["canonical_store_facts"] == reopen["canonical_store_facts"]
        and terminal["run_view_sha256"] == reopen["run_view_sha256"]
        and terminal["events_sha256"] == reopen["events_sha256"],
        "completed_observation_reopen_mismatch",
    )


def run_journal_contract_checks() -> None:
    with tempfile.TemporaryDirectory(
        prefix="codewhale-m7g2-journal-contract-"
    ) as raw_directory:
        directory = Path(raw_directory)
        output = directory / "complete.jsonl"
        with Journal.claim(output) as journal:
            journal.emit(
                {
                    "record_type": "plan",
                    "product_metric_eligible": False,
                    "key_accessed": False,
                    "network_accessed": False,
                }
            )
            journal.emit(terminal_snapshot_payload())
            journal.emit(reopen_snapshot_payload())
            journal.emit(verifier_snapshot_payload())
            journal.emit(
                {
                    "record_type": "arm_result",
                    "observation_id": "offline-observation-1",
                    "product_metric_eligible": False,
                    "key_accessed": False,
                    "network_accessed": False,
                }
            )
        audit = read_journal(output, allow_partial_tail=False)
        validate_completed_observation_order(audit)
        try:
            Journal.claim(output)
        except EvaluationError as error:
            require(
                error.code == "output_claim_failed",
                "existing_output_rejection_invalid",
            )
        else:
            raise EvaluationError("existing_output_was_reopened")

        tampered = directory / "tampered.jsonl"
        lines = output.read_bytes().splitlines()
        value = json.loads(lines[1])
        value["payload"]["canonical_store_facts"]["run_view"]["tool_calls"] = 1
        lines[1] = canonical_bytes(value)
        tampered.write_bytes(b"\n".join(lines) + b"\n")
        os.chmod(tampered, 0o600)
        try:
            read_journal(tampered, allow_partial_tail=False)
        except EvaluationError as error:
            require(
                error.code == "journal_hash_invalid",
                "tamper_rejection_invalid",
            )
        else:
            raise EvaluationError("journal_tamper_accepted")


def inspect_fault_result(
    fault: str,
    path: Path,
    returncode: int,
) -> dict[str, Any]:
    audit = read_journal(path, allow_partial_tail=True)
    types = payload_types(audit)
    require(types and types[0] == "plan", "fault_plan_missing")
    require("arm_result" not in types, "fault_derived_result_committed")
    killed = fault.endswith("_kill")
    require(
        (killed and returncode == -signal.SIGKILL)
        or (not killed and returncode == 2),
        "fault_exit_invalid",
        {"fault": fault, "returncode": returncode},
    )
    if fault == "terminal_before_write_kill":
        require(types == ["plan"], "fault_terminal_order_invalid")
    elif fault == "terminal_mid_write_kill":
        require(
            types == ["plan"] and audit["partial_tail_bytes"] > 0,
            "fault_partial_write_invalid",
        )
    else:
        require(
            "terminal_snapshot" in types,
            "fault_terminal_snapshot_missing",
        )
    if fault in {
        "after_reopen_fsync_kill",
        "after_verifier_fsync_kill",
        "before_result_kill",
        "identity_exception",
        "verifier_exception",
        "surface_exception",
        "accounting_exception",
    }:
        require(
            "sqlite_reopen_snapshot" in types,
            "fault_reopen_snapshot_missing",
        )
    if fault in {
        "after_verifier_fsync_kill",
        "before_result_kill",
        "identity_exception",
        "verifier_exception",
        "surface_exception",
        "accounting_exception",
    }:
        require(
            "verifier_snapshot" in types,
            "fault_verifier_snapshot_missing",
        )
    if not killed:
        require(types[-1] == "abort", "fault_abort_missing")
    return {
        "fault": fault,
        "returncode": returncode,
        "record_types": types,
        "partial_tail_bytes": audit["partial_tail_bytes"],
        "journal_sha256": audit["file_sha256"],
        "passed": True,
    }


def run_fault_matrix() -> list[dict[str, Any]]:
    results: list[dict[str, Any]] = []
    with tempfile.TemporaryDirectory(
        prefix="codewhale-m7g2-observer-fault-"
    ) as raw_directory:
        directory = Path(raw_directory)
        for fault in FAULTS:
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
                ],
                cwd=ROOT,
                env=safe_env(),
                stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=30,
                check=False,
            )
            require(not completed.stdout, "fault_child_stdout_invalid")
            require(not completed.stderr, "fault_child_stderr_invalid")
            results.append(
                inspect_fault_result(
                    fault,
                    output,
                    completed.returncode,
                )
            )
    return results


def legacy_raw_evidence() -> dict[str, Any]:
    metadata = LEGACY_RAW_PATH.lstat()
    require(
        stat.S_ISREG(metadata.st_mode)
        and not LEGACY_RAW_PATH.is_symlink()
        and stat.S_IMODE(metadata.st_mode) == 0o600,
        "legacy_raw_identity_invalid",
    )
    require(
        file_hash(LEGACY_RAW_PATH) == LEGACY_RAW_SHA256,
        "legacy_raw_hash_invalid",
    )
    records: list[dict[str, Any]] = []
    for line in LEGACY_RAW_PATH.read_bytes().splitlines():
        try:
            value = json.loads(line)
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise EvaluationError("legacy_raw_invalid") from error
        require(isinstance(value, dict), "legacy_raw_invalid")
        records.append(value)
    types = [record.get("record_type") for record in records]
    require(types == ["plan", "credential_access", "abort"], "legacy_raw_shape_invalid")
    abort = records[-1]
    require(
        abort.get("completed_arms") == 0
        and abort.get("error_code") == "run_identity_invalid"
        and abort.get("key_accessed") is True
        and abort.get("network_accessed") is True
        and abort.get("maximum_reruns") == 0
        and "accounting" not in abort
        and not abort.get("details"),
        "legacy_raw_boundary_invalid",
    )
    return {
        "record_type": "legacy_suite_exclusion",
        "path": str(LEGACY_RAW_PATH.relative_to(ROOT)),
        "sha256": LEGACY_RAW_SHA256,
        "mode": "0600",
        "records": len(records),
        "completed_arms": 0,
        "billing": "unknown",
        "continuation_forbidden": True,
        "sample_splicing_forbidden": True,
        "product_metric_eligible": False,
        "key_accessed": False,
        "network_accessed": False,
    }


def load_manifest(*, frozen: bool) -> dict[str, Any]:
    try:
        manifest = json.loads(MANIFEST_PATH.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise EvaluationError("manifest_unavailable") from error
    require(
        isinstance(manifest, dict)
        and manifest.get("schema") == MANIFEST_SCHEMA
        and manifest.get("admission", {}).get("live_api_admitted") is False
        and manifest.get("resources", {}).get("maximum_reruns") == 0
        and manifest.get("fault_injection", {}).get("faults") == list(FAULTS),
        "manifest_identity_invalid",
    )
    legacy = manifest.get("legacy_suite", {})
    require(
        legacy.get("raw_sha256") == LEGACY_RAW_SHA256
        and legacy.get("billing") == "unknown"
        and legacy.get("continuation_forbidden") is True,
        "manifest_legacy_boundary_invalid",
    )
    if frozen:
        hashes = manifest.get("frozen_hashes", {})
        without_hashes = dict(manifest)
        without_hashes.pop("frozen_hashes", None)
        require(
            hashes.get("harness_sha256")
            == file_hash(Path(__file__).resolve())
            and hashes.get("manifest_content_sha256_excluding_frozen_hashes")
            == canonical_hash(without_hashes)
            and hashes.get("sample_store_facts_sha256")
            == canonical_hash(sample_store_facts())
            and hashes.get("fault_matrix_sha256")
            == canonical_hash(list(FAULTS)),
            "frozen_hash_mismatch",
        )
    return manifest


def freeze_report() -> dict[str, Any]:
    manifest = load_manifest(frozen=False)
    without_hashes = dict(manifest)
    without_hashes.pop("frozen_hashes", None)
    return {
        "harness_sha256": file_hash(Path(__file__).resolve()),
        "manifest_content_sha256_excluding_frozen_hashes": canonical_hash(
            without_hashes
        ),
        "sample_store_facts_sha256": canonical_hash(sample_store_facts()),
        "fault_matrix_sha256": canonical_hash(list(FAULTS)),
        "legacy_raw_sha256": file_hash(LEGACY_RAW_PATH),
    }


def run_self_test() -> int:
    load_manifest(frozen=True)
    terminal = terminal_snapshot_payload()
    reopen = reopen_snapshot_payload()
    require(
        terminal["canonical_store_facts"] == reopen["canonical_store_facts"]
        and terminal["run_view_sha256"] == reopen["run_view_sha256"]
        and terminal["events_sha256"] == reopen["events_sha256"],
        "sample_reopen_identity_invalid",
    )
    run_journal_contract_checks()
    faults = run_fault_matrix()
    print(
        json.dumps(
            {
                "schema": JOURNAL_SCHEMA,
                "record_type": "self_test",
                "passed": True,
                "faults": len(faults),
                "fault_matrix_sha256": canonical_hash(faults),
                "key_accessed": False,
                "network_accessed": False,
            },
            ensure_ascii=False,
            sort_keys=True,
        )
    )
    return 0


def run_offline(output: Path) -> int:
    manifest = load_manifest(frozen=True)
    require(
        output.resolve().parent == (ROOT / "eval/results").resolve()
        and output.suffix == ".jsonl",
        "offline_output_scope_invalid",
    )
    with Journal.claim(output.resolve()) as journal:
        journal.emit(
            {
                "record_type": "plan",
                "suite_id": manifest["suite_id"],
                "source_identity": manifest["source_identity"],
                "manifest_sha256": file_hash(MANIFEST_PATH),
                "harness_sha256": file_hash(Path(__file__).resolve()),
                "product_metric_eligible": False,
                "maximum_reruns": 0,
                "key_accessed": False,
                "network_accessed": False,
            }
        )
        journal.emit(legacy_raw_evidence())
        faults = run_fault_matrix()
        for result in faults:
            journal.emit(
                {
                    "record_type": "fault_result",
                    **result,
                    "product_metric_eligible": False,
                    "key_accessed": False,
                    "network_accessed": False,
                }
            )
        journal.emit(
            {
                "record_type": "offline_summary",
                "faults_passed": len(faults),
                "faults_total": len(FAULTS),
                "observer_contract": (
                    "terminal_snapshot -> sqlite_reopen_snapshot -> "
                    "verifier_snapshot -> derived arm_result"
                ),
                "decision": "hold_no_new_production_delta",
                "successor_live_api_admitted": False,
                "reason": (
                    "observer-only durability is not a new production "
                    "read-only fan-out treatment"
                ),
                "product_metric_eligible": False,
                "maximum_reruns": 0,
                "key_accessed": False,
                "network_accessed": False,
            }
        )
    audit = read_journal(output.resolve(), allow_partial_tail=False)
    require(
        payload_types(audit)[-1] == "offline_summary"
        and audit["partial_tail_bytes"] == 0,
        "offline_output_invalid",
    )
    print(
        json.dumps(
            {
                "schema": JOURNAL_SCHEMA,
                "record_type": "offline_complete",
                "output": str(output.resolve()),
                "records": len(audit["records"]),
                "sha256": audit["file_sha256"],
                "mode": "0600",
                "key_accessed": False,
                "network_accessed": False,
            },
            ensure_ascii=False,
            sort_keys=True,
        )
    )
    return 0


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--self-test", action="store_true")
    mode.add_argument("--freeze-report", action="store_true")
    mode.add_argument("--offline", action="store_true")
    mode.add_argument("--fault-child", choices=FAULTS)
    parser.add_argument("--output", type=Path)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        if args.freeze_report:
            print(
                json.dumps(
                    freeze_report(),
                    ensure_ascii=False,
                    indent=2,
                    sort_keys=True,
                )
            )
            return 0
        if args.self_test:
            require(args.output is None, "self_test_output_forbidden")
            return run_self_test()
        require(args.output is not None, "output_required")
        if args.fault_child:
            return run_fault_child(args.fault_child, args.output.resolve())
        return run_offline(args.output.resolve())
    except EvaluationError as error:
        print(
            json.dumps(
                {
                    "schema": JOURNAL_SCHEMA,
                    "record_type": "error",
                    "error_code": error.code,
                    "details": error.details,
                    "key_accessed": False,
                    "network_accessed": False,
                },
                ensure_ascii=False,
                sort_keys=True,
            ),
            file=sys.stderr,
        )
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
