#!/usr/bin/env python3
"""Corrected fixed-Pro coding regression and loss-acquisition Harness.

The default M9-C campaign remains byte-addressed to its frozen successor
contract. ``--campaign m11`` selects the multi-language M11 loss baseline,
and ``--campaign m12`` selects the corrected terminal-convergence reproduction.
``--campaign m15`` selects the fresh position-1 current product-loss
acquisition after M14 observer conformance.
``--campaign m18`` selects the first DSE-native position-1 local reliability
baseline after the bilingual identity cutover.
``--campaign m19`` selects the fresh DSE local coding-reliability acquisition
whose outer watchdog preserves a credential-free Store/accounting boundary.
``--campaign m20b`` selects the fresh post-transport-viability fixed-Pro
reliability acquisition without rerunning or completing M19.
``--campaign m23b`` selects the private 20-task Hardness control-only
contract. Its self-test and freeze report are credential-free; live acquisition
remains separately admitted and is never implied by fixture conformance.
``--campaign m30`` selects the current one-arm-per-task dogfood loss
acquisition while inheriting only the frozen M23 task material.
``--campaign m36a2`` selects the fresh three-task explicit Writer loss
confirmation set. It never counts historical M36 raw toward the repeated-loss
threshold and admits no production treatment by itself.
``--transport-viability`` runs the M20 non-inference official host/account
reachability boundary through the migrated DSE Doctor caller.
``--observer-conformance`` runs the credential-free M14 tool/lifecycle corpus.
``--acceptance-conformance`` runs the credential-free M16 acceptance-
equivalence corpus. ``--interaction-conformance`` runs the credential-free
M38 typed interaction and durable observer-abort corpus.
``--truth-conformance`` runs the credential-free M23
behavior/accounting orthogonality corpus. ``--hardness-conformance`` runs the
credential-free M23-B2 metric and real mid-run continuity observer corpus. Live
campaigns exercise temporary Git repositories through canonical
``dse app-server --stdio`` and record terminal and RunStore facts before
credential-free reopen, deterministic verification, or label derivation. They
are regression label collectors, not product A/Bs.
"""

from __future__ import annotations

import argparse
from collections import Counter, defaultdict
import copy
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import selectors
import shutil
import signal
import sqlite3
import stat
import subprocess
import sys
import tempfile
import threading
import time
from typing import Any, BinaryIO
import uuid


ROOT = Path(__file__).resolve().parents[1]
M20_MANIFEST_PATH = (
    ROOT / "eval/manifests/m20-transport-viability-v1.json"
)
M20_MANIFEST_SCHEMA = "dse.eval.m20-transport-viability.v1"
M20_ADMISSION_SCHEMA = "dse.eval.m20-transport-viability-live-admission.v1"
M20_JOURNAL_SCHEMA = "dse.eval.m20-transport-viability-journal.v1"
M20_SUCCESS_MARKER = b"Official API host and credential are reachable"
M20_FAILURE_MARKER = b"API connection failed"


def selected_campaign(arguments: list[str]) -> str:
    selected = []
    for index, argument in enumerate(arguments):
        if argument == "--campaign":
            selected.append(
                arguments[index + 1]
                if index + 1 < len(arguments)
                else "invalid"
            )
        elif argument.startswith("--campaign="):
            selected.append(argument.partition("=")[2])
    if not selected:
        return "m9c"
    if len(selected) != 1 or selected[0] not in {
        "m9c",
        "m11",
        "m12",
        "m15",
        "m18",
        "m19",
        "m20b",
        "m23b",
        "m30",
        "m36a2",
    }:
        # Let argparse reject retired or unknown campaign names after the
        # credential-free default contract has loaded.
        return "m9c"
    return selected[0]


CAMPAIGN = selected_campaign(sys.argv[1:])
CURRENT_LOSS_CAMPAIGNS = {
    "m11",
    "m12",
    "m15",
    "m18",
    "m19",
    "m20b",
    "m23b",
    "m30",
    "m36a2",
}
VERIFIER_ENVIRONMENT_CAMPAIGNS = {
    "m12",
    "m15",
    "m18",
    "m19",
    "m20b",
    "m23b",
    "m30",
    "m36a2",
}
DSE_CAMPAIGNS = {"m18", "m19", "m20b", "m23b", "m30", "m36a2"}
HARDNESS_CAMPAIGNS = {"m23b", "m30"}
WRITER_CONFIRMATION_CAMPAIGNS = {"m36a2"}
PROJECT_TASK_CAMPAIGNS = HARDNESS_CAMPAIGNS | WRITER_CONFIRMATION_CAMPAIGNS
LOSS_TRUTH_CAMPAIGNS = {"m30", "m36a2"}
CURRENT_PERMISSION_CAMPAIGNS = {"m30", "m36a2"}
if CAMPAIGN == "m36a2":
    MANIFEST_PATH = (
        ROOT / "eval/manifests/m36a2-writer-loss-confirmation-v1.json"
    )
    BASE_MANIFEST_PATH: Path | None = None
    MANIFEST_SCHEMA = "dse.eval.m36a2-writer-loss-confirmation.v1"
    BASE_MANIFEST_SCHEMA: str | None = None
    JOURNAL_SCHEMA = (
        "dse.eval.m36a2-writer-loss-confirmation-journal.v1"
    )
    ADMISSION_SCHEMA = (
        "dse.eval.m36a2-writer-loss-confirmation-live-admission.v1"
    )
    RUN_API = 15
    EVENT_API = 22
    STATE_SCHEMA = 28
    EXEC_STREAM = 6
elif CAMPAIGN == "m30":
    MANIFEST_PATH = (
        ROOT / "eval/manifests/m30-dogfood-loss-acquisition-v1.json"
    )
    BASE_MANIFEST_PATH = (
        ROOT / "eval/manifests/m23b-hardness-control-v1.json"
    )
    MANIFEST_SCHEMA = "dse.eval.m30-dogfood-loss-acquisition.v1"
    BASE_MANIFEST_SCHEMA = "dse.eval.m23b-hardness-control.v1"
    JOURNAL_SCHEMA = "dse.eval.m30-dogfood-loss-acquisition-journal.v1"
    ADMISSION_SCHEMA = (
        "dse.eval.m30-dogfood-loss-acquisition-live-admission.v1"
    )
    RUN_API = 13
    EVENT_API = 20
    STATE_SCHEMA = 26
    EXEC_STREAM = 4
elif CAMPAIGN == "m23b":
    MANIFEST_PATH = (
        ROOT / "eval/manifests/m23b-hardness-control-v1.json"
    )
    BASE_MANIFEST_PATH: Path | None = None
    MANIFEST_SCHEMA = "dse.eval.m23b-hardness-control.v1"
    BASE_MANIFEST_SCHEMA: str | None = None
    JOURNAL_SCHEMA = "dse.eval.m23b-hardness-control-journal.v1"
    ADMISSION_SCHEMA = "dse.eval.m23b-hardness-control-live-admission.v1"
    RUN_API = 12
    EVENT_API = 19
    STATE_SCHEMA = 25
    EXEC_STREAM = 4
elif CAMPAIGN == "m20b":
    MANIFEST_PATH = (
        ROOT / "eval/manifests/m20b-fixed-pro-reliability-v1.json"
    )
    BASE_MANIFEST_PATH: Path | None = None
    MANIFEST_SCHEMA = "dse.eval.m20b-fixed-pro-reliability.v1"
    BASE_MANIFEST_SCHEMA: str | None = None
    JOURNAL_SCHEMA = "dse.eval.m20b-fixed-pro-reliability-journal.v1"
    ADMISSION_SCHEMA = (
        "dse.eval.m20b-fixed-pro-reliability-live-admission.v1"
    )
    RUN_API = 12
    EVENT_API = 19
    STATE_SCHEMA = 25
    EXEC_STREAM = 4
elif CAMPAIGN == "m19":
    MANIFEST_PATH = (
        ROOT / "eval/manifests/m19-local-reliability-baseline-v1.json"
    )
    BASE_MANIFEST_PATH: Path | None = None
    MANIFEST_SCHEMA = "dse.eval.m19-local-reliability-baseline.v1"
    BASE_MANIFEST_SCHEMA: str | None = None
    JOURNAL_SCHEMA = "dse.eval.m19-local-reliability-journal.v1"
    ADMISSION_SCHEMA = "dse.eval.m19-local-reliability-live-admission.v1"
    RUN_API = 12
    EVENT_API = 19
    STATE_SCHEMA = 25
    EXEC_STREAM = 4
elif CAMPAIGN == "m18":
    MANIFEST_PATH = (
        ROOT / "eval/manifests/m18-local-reliability-baseline-v1.json"
    )
    BASE_MANIFEST_PATH: Path | None = None
    MANIFEST_SCHEMA = "dse.eval.m18-local-reliability-baseline.v1"
    BASE_MANIFEST_SCHEMA: str | None = None
    JOURNAL_SCHEMA = "dse.eval.m18-local-reliability-journal.v1"
    ADMISSION_SCHEMA = "dse.eval.m18-local-reliability-live-admission.v1"
    RUN_API = 12
    EVENT_API = 19
    STATE_SCHEMA = 25
    EXEC_STREAM = 4
elif CAMPAIGN == "m15":
    MANIFEST_PATH = (
        ROOT / "eval/manifests/m15-product-loss-acquisition-v1.json"
    )
    BASE_MANIFEST_PATH: Path | None = None
    MANIFEST_SCHEMA = "codewhale.eval.m15-product-loss-acquisition.v1"
    BASE_MANIFEST_SCHEMA: str | None = None
    JOURNAL_SCHEMA = "codewhale.eval.m15-product-loss-acquisition-journal.v1"
    ADMISSION_SCHEMA = (
        "codewhale.eval.m15-product-loss-acquisition-live-admission.v1"
    )
    RUN_API = 12
    EVENT_API = 18
    STATE_SCHEMA = 24
    EXEC_STREAM = 3
elif CAMPAIGN == "m12":
    MANIFEST_PATH = (
        ROOT
        / "eval/manifests/m12-terminal-convergence-reproduction-v1.json"
    )
    BASE_MANIFEST_PATH: Path | None = None
    MANIFEST_SCHEMA = (
        "codewhale.eval.m12-terminal-convergence-reproduction.v1"
    )
    BASE_MANIFEST_SCHEMA: str | None = None
    JOURNAL_SCHEMA = (
        "codewhale.eval.m12-terminal-convergence-reproduction-journal.v1"
    )
    ADMISSION_SCHEMA = (
        "codewhale.eval.m12-terminal-convergence-live-admission.v1"
    )
    RUN_API = 12
    EVENT_API = 18
    STATE_SCHEMA = 24
    EXEC_STREAM = 3
elif CAMPAIGN == "m11":
    MANIFEST_PATH = ROOT / "eval/manifests/m11-loss-baseline-v1.json"
    BASE_MANIFEST_PATH: Path | None = None
    MANIFEST_SCHEMA = "codewhale.eval.m11-loss-baseline.v1"
    BASE_MANIFEST_SCHEMA: str | None = None
    JOURNAL_SCHEMA = "codewhale.eval.m11-loss-baseline-journal.v1"
    ADMISSION_SCHEMA = "codewhale.eval.m11-loss-baseline-live-admission.v1"
    RUN_API = 12
    EVENT_API = 18
    STATE_SCHEMA = 24
    EXEC_STREAM = 3
else:
    MANIFEST_PATH = (
        ROOT / "eval/manifests/m9-c-fixed-pro-regression-successor-v1.json"
    )
    BASE_MANIFEST_PATH = (
        ROOT / "eval/manifests/m9-b-fixed-pro-regression-v1.json"
    )
    MANIFEST_SCHEMA = "codewhale.eval.m9-c-fixed-pro-regression-successor.v1"
    BASE_MANIFEST_SCHEMA = "codewhale.eval.m9-b-fixed-pro-regression.v1"
    JOURNAL_SCHEMA = (
        "codewhale.eval.m9-c-fixed-pro-regression-successor-journal.v1"
    )
    ADMISSION_SCHEMA = (
        "codewhale.eval.m9-c-fixed-pro-regression-live-admission.v1"
    )
    RUN_API = 11
    EVENT_API = 17
    STATE_SCHEMA = 23
    EXEC_STREAM = 3
if CAMPAIGN == "m36a2":
    TRAJECTORY_MANIFEST_PATH = (
        ROOT / "eval/manifests/m36a2-writer-loss-analysis-v1.json"
    )
    TRAJECTORY_MANIFEST_SCHEMA = (
        "dse.eval.m36a2-writer-loss-analysis.v1"
    )
    TRAJECTORY_REPORT_SCHEMA = (
        "dse.eval.m36a2-writer-loss-report.v1"
    )
elif CAMPAIGN == "m30":
    TRAJECTORY_MANIFEST_PATH = (
        ROOT / "eval/manifests/m30-dogfood-loss-analysis-v1.json"
    )
    TRAJECTORY_MANIFEST_SCHEMA = (
        "dse.eval.m30-dogfood-loss-analysis.v1"
    )
    TRAJECTORY_REPORT_SCHEMA = "dse.eval.m30-dogfood-loss-report.v1"
elif CAMPAIGN == "m23b":
    TRAJECTORY_MANIFEST_PATH = (
        ROOT / "eval/manifests/m23b-hardness-control-analysis-v1.json"
    )
    TRAJECTORY_MANIFEST_SCHEMA = (
        "dse.eval.m23b-hardness-control-analysis.v1"
    )
    TRAJECTORY_REPORT_SCHEMA = "dse.eval.m23b-hardness-control-report.v1"
elif CAMPAIGN == "m20b":
    TRAJECTORY_MANIFEST_PATH = (
        ROOT / "eval/manifests/m20b-fixed-pro-reliability-analysis-v1.json"
    )
    TRAJECTORY_MANIFEST_SCHEMA = (
        "dse.eval.m20b-fixed-pro-reliability-analysis.v1"
    )
    TRAJECTORY_REPORT_SCHEMA = (
        "dse.eval.m20b-fixed-pro-reliability-report.v1"
    )
elif CAMPAIGN == "m19":
    TRAJECTORY_MANIFEST_PATH = (
        ROOT / "eval/manifests/m19-local-reliability-analysis-v1.json"
    )
    TRAJECTORY_MANIFEST_SCHEMA = (
        "dse.eval.m19-local-reliability-analysis.v1"
    )
    TRAJECTORY_REPORT_SCHEMA = "dse.eval.m19-local-reliability-report.v1"
elif CAMPAIGN == "m18":
    TRAJECTORY_MANIFEST_PATH = (
        ROOT / "eval/manifests/m18-local-reliability-analysis-v1.json"
    )
    TRAJECTORY_MANIFEST_SCHEMA = (
        "dse.eval.m18-local-reliability-analysis.v1"
    )
    TRAJECTORY_REPORT_SCHEMA = "dse.eval.m18-local-reliability-report.v1"
elif CAMPAIGN == "m15":
    TRAJECTORY_MANIFEST_PATH = (
        ROOT / "eval/manifests/m15-product-loss-analysis-v1.json"
    )
    TRAJECTORY_MANIFEST_SCHEMA = (
        "codewhale.eval.m15-product-loss-analysis.v1"
    )
    TRAJECTORY_REPORT_SCHEMA = (
        "codewhale.eval.m15-product-loss-report.v1"
    )
elif CAMPAIGN == "m12":
    TRAJECTORY_MANIFEST_PATH = (
        ROOT
        / "eval/manifests/m12-terminal-convergence-analysis-v1.json"
    )
    TRAJECTORY_MANIFEST_SCHEMA = (
        "codewhale.eval.m12-terminal-convergence-analysis.v1"
    )
    TRAJECTORY_REPORT_SCHEMA = (
        "codewhale.eval.m12-terminal-convergence-report.v1"
    )
elif CAMPAIGN == "m11":
    TRAJECTORY_MANIFEST_PATH = (
        ROOT / "eval/manifests/m11-trajectory-loss-analysis-v1.json"
    )
    TRAJECTORY_MANIFEST_SCHEMA = (
        "codewhale.eval.m11-trajectory-loss-analysis.v1"
    )
    TRAJECTORY_REPORT_SCHEMA = (
        "codewhale.eval.m11-trajectory-loss-report.v1"
    )
else:
    TRAJECTORY_MANIFEST_PATH = (
        ROOT / "eval/manifests/m10-f-trajectory-loss-analyzer-v1.json"
    )
    TRAJECTORY_MANIFEST_SCHEMA = (
        "codewhale.eval.m10-f-trajectory-loss-analyzer.v1"
    )
    TRAJECTORY_REPORT_SCHEMA = (
        "codewhale.eval.m10-f-trajectory-loss-report.v1"
    )
OBSERVER_MANIFEST_PATH = (
    ROOT / "eval/manifests/m14-observer-conformance-v1.json"
)
OBSERVER_MANIFEST_SCHEMA = "codewhale.eval.m14-observer-conformance.v1"
OBSERVER_CORPUS_SCHEMA = (
    "codewhale.eval.m14-observer-conformance-corpus.v1"
)
ACCEPTANCE_MANIFEST_PATH = (
    ROOT / "eval/manifests/m16-acceptance-equivalence-observer-v1.json"
)
ACCEPTANCE_MANIFEST_SCHEMA = (
    "codewhale.eval.m16-acceptance-equivalence-observer.v1"
)
ACCEPTANCE_CORPUS_SCHEMA = (
    "codewhale.eval.m16-acceptance-equivalence-observer-corpus.v1"
)
INTERACTION_MANIFEST_PATH = (
    ROOT / "eval/manifests/m38-typed-interaction-observer-v1.json"
)
INTERACTION_MANIFEST_SCHEMA = (
    "dse.eval.m38-typed-interaction-observer.v1"
)
INTERACTION_CORPUS_SCHEMA = (
    "dse.eval.m38-typed-interaction-observer-corpus.v1"
)
INTERACTION_REPORT_SCHEMA = (
    "dse.eval.m38-typed-interaction-observer-report.v1"
)
TRUTH_MANIFEST_PATH = (
    ROOT / "eval/manifests/m23-behavior-accounting-truth-v1.json"
)
TRUTH_MANIFEST_SCHEMA = "dse.eval.m23-behavior-accounting-truth.v1"
TRUTH_CORPUS_SCHEMA = (
    "dse.eval.m23-behavior-accounting-truth-corpus.v1"
)
TRUTH_REPORT_SCHEMA = (
    "dse.eval.m23-behavior-accounting-truth-report.v1"
)
HARDNESS_OBSERVER_MANIFEST_PATH = (
    ROOT / "eval/manifests/m23b-hardness-metrics-observer-v1.json"
)
HARDNESS_OBSERVER_MANIFEST_SCHEMA = (
    "dse.eval.m23b-hardness-metrics-observer.v1"
)
HARDNESS_OBSERVER_CORPUS_SCHEMA = (
    "dse.eval.m23b-hardness-metrics-observer-corpus.v1"
)
HARDNESS_OBSERVER_REPORT_SCHEMA = (
    "dse.eval.m23b-hardness-metrics-observer-report.v1"
)
HARDNESS_CONTINUITY_MANIFEST_PATH = (
    ROOT / "eval/manifests/m23b-hardness-live-continuity-v1.json"
)
HARDNESS_CONTINUITY_MANIFEST_SCHEMA = (
    "dse.eval.m23b-hardness-live-continuity.v1"
)
HARDNESS_CONTINUITY_REPORT_SCHEMA = (
    "dse.eval.m30-dogfood-continuity-report.v1"
    if CAMPAIGN == "m30"
    else "dse.eval.m23b-hardness-live-continuity-report.v1"
)
BEHAVIOR_STATUSES = {
    "verified_success",
    "correct_safety_rejection",
    "verified_product_failure",
    "measurement_interruption",
    "invalid",
}
ACCOUNTING_STATUSES = {
    "complete",
    "usage_incomplete",
    "billing_unknown",
    "unpriced",
}
M15_REFERENCE_PATCH_PATH = (
    ROOT / "eval/fixtures/m15-product-loss-reference.patch"
)
M18_REFERENCE_PATCH_PATH = (
    ROOT / "eval/fixtures/m18-local-reliability-reference.patch"
)
M19_REFERENCE_PATCH_PATH = (
    ROOT / "eval/fixtures/m19-local-reliability-reference.patch"
)
M20B_REFERENCE_PATCH_PATH = (
    ROOT / "eval/fixtures/m20b-local-reliability-reference.patch"
)
STABLE_TOOL_OUTCOME_FIELDS = (
    "failure_code",
    "invocation",
    "transport",
    "operation",
    "side_effect",
    "retry",
)
MODEL = "deepseek-v4-pro"
REASONING = "high"
ZERO_HASH = "sha256:" + ("0" * 64)
MAX_FRAME = 16 * 1024 * 1024
WRITER_LIFECYCLE = (
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
WRITER_ONLY_LIFECYCLE = (
    "agent_workspace_created",
    "agent_seal_prepared",
    "agent_seal_committed",
    "agent_integration_prepared",
    "agent_integration_started",
    "agent_integration_committed",
    "agent_cleanup_prepared",
    "agent_cleanup_committed",
)
MAY_WRITE_TOOLS = {"apply_patch", "edit_file"}
USAGE_FIELDS = (
    "input_tokens",
    "output_tokens",
    "cache_hit_tokens",
    "cache_miss_tokens",
    "cache_write_tokens",
    "reasoning_tokens",
    "reasoning_replay_tokens",
)
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


class EvaluationError(RuntimeError):
    """Fail-closed evaluator or evidence error."""

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


def canonical_hash(value: Any) -> str:
    return sha256_bytes(canonical_bytes(value))


def file_hash(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return "sha256:" + digest.hexdigest()


def repository_relative(path: Path, failure_code: str) -> str:
    try:
        return path.relative_to(ROOT).as_posix()
    except ValueError as error:
        raise EvaluationError(failure_code) from error


def read_json_object(path: Path, failure_code: str) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise EvaluationError(failure_code) from error
    require(isinstance(value, dict), failure_code)
    return value


def canonical_tree_hash(root: Path) -> str:
    require(root.is_dir() and not root.is_symlink(), "fixture_shape_invalid")
    entries: list[dict[str, Any]] = []
    for path in sorted(root.rglob("*")):
        relative = path.relative_to(root)
        if not path.is_file() or ".git" in relative.parts:
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
    return canonical_hash(entries)


def load_manifest() -> dict[str, Any]:
    require(
        CAMPAIGN
        in {
            "m9c",
            "m11",
            "m12",
            "m15",
            "m18",
            "m19",
            "m20b",
            "m23b",
            "m30",
            "m36a2",
        },
        "campaign_invalid",
    )
    if CAMPAIGN in CURRENT_LOSS_CAMPAIGNS:
        manifest = read_json_object(MANIFEST_PATH, "manifest_unavailable")
        require(
            manifest.get("schema") == MANIFEST_SCHEMA,
            "manifest_schema_invalid",
        )
        source = manifest.get("source_identity", {})
        resources = manifest.get("resources", {})
        tasks = manifest.get("tasks")
        tool_policies = manifest.get("tool_policies")
        if CAMPAIGN == "m30":
            inherited = manifest.get("inherited_contract")
            require(
                isinstance(inherited, dict)
                and BASE_MANIFEST_PATH is not None
                and BASE_MANIFEST_SCHEMA is not None
                and inherited.get("path")
                == BASE_MANIFEST_PATH.relative_to(ROOT).as_posix()
                and inherited.get("file_sha256")
                == file_hash(BASE_MANIFEST_PATH)
                and inherited.get("sections")
                == [
                    "fixture_contract",
                    "reference_patches",
                    "tasks",
                    "tool_policies",
                ]
                and inherited.get("historical_identity_inherited") is False
                and inherited.get("historical_raw_is_input") is False,
                "inherited_contract_invalid",
            )
            base = read_json_object(
                BASE_MANIFEST_PATH, "inherited_manifest_unavailable"
            )
            require(
                base.get("schema") == BASE_MANIFEST_SCHEMA,
                "inherited_manifest_schema_invalid",
            )
            manifest = {
                **manifest,
                "fixture_contract": base.get("fixture_contract"),
                "reference_patches": base.get("reference_patches"),
                "tasks": json.loads(
                    json.dumps(base.get("tasks"), ensure_ascii=False)
                ),
                "tool_policies": json.loads(
                    json.dumps(
                        base.get("tool_policies"), ensure_ascii=False
                    )
                ),
            }
            tasks = manifest["tasks"]
            tool_policies = manifest["tool_policies"]
        if CAMPAIGN in WRITER_CONFIRMATION_CAMPAIGNS:
            expected_tasks = [
                "writer_retry_ledger",
                "writer_header_policy",
                "writer_route_contract",
            ]
            require(
                source.get("run_api") == RUN_API
                and source.get("runtime_event") == EVENT_API
                and source.get("state_schema") == STATE_SCHEMA
                and source.get("exec_stream") == EXEC_STREAM,
                "protocol_identity_invalid",
            )
            require(
                resources.get("model") == MODEL
                and resources.get("reasoning_effort") == REASONING
                and resources.get("runs_per_task") == 1
                and resources.get("formal_tasks") == len(expected_tasks)
                and resources.get("formal_arms") == len(expected_tasks)
                and resources.get("maximum_reruns") == 0
                and resources.get("max_runtime_retries_per_arm") == 2
                and resources.get("permission_mode") == "agent"
                and resources.get("interactive") is False,
                "resource_identity_invalid",
            )
            require(
                isinstance(resources.get("runtime_wall_time_ms"), int)
                and isinstance(resources.get("harness_wall_time_ms"), int)
                and resources["harness_wall_time_ms"]
                >= resources["runtime_wall_time_ms"] + 30_000,
                "deadline_resource_identity_invalid",
            )
            require(
                manifest.get("metrics")
                == [
                    "verified_success",
                    "false_success",
                    "behavior_status",
                    "behavior_owner_code",
                    "behavior_loss_code",
                    "accounting_status",
                    "request_count",
                    "input_tokens",
                    "output_tokens",
                    "cache_hit_tokens",
                    "cache_miss_tokens",
                    "cost_nanousd",
                    "wall_time_ms",
                    "writer_lifecycle",
                    "integrated_files",
                    "cleanup_proof",
                    "latest_revision_receipt",
                ],
                "writer_confirmation_metric_contract_invalid",
            )
            require(
                isinstance(tasks, dict)
                and list(tasks) == expected_tasks
                and isinstance(tool_policies, dict),
                "writer_confirmation_task_identity_invalid",
            )
            fixture = manifest.get("fixture_contract")
            require(
                isinstance(fixture, dict)
                and fixture.get("path")
                == "eval/fixtures/m36a2-writer-monorepo"
                and fixture.get("tree_sha256")
                == canonical_tree_hash(ROOT / fixture["path"])
                and fixture.get("base_commit")
                == "26cad8ce4ecdb1c79a13acd8b6d8d1f54c1fe0ec"
                and fixture.get("commit_profile")
                == "m36a2-2026-07-27",
                "writer_confirmation_fixture_contract_invalid",
            )
            references = manifest.get("reference_patches")
            require(
                isinstance(references, dict)
                and set(references) == set(expected_tasks),
                "writer_confirmation_reference_identity_invalid",
            )
            for reference in references.values():
                require(
                    isinstance(reference, dict)
                    and isinstance(reference.get("path"), str)
                    and reference.get("sha256")
                    == file_hash(ROOT / reference["path"]),
                    "writer_confirmation_reference_identity_invalid",
                )
            expanded_tasks: dict[str, dict[str, Any]] = {}
            projects: set[str] = set()
            for task_id, task in tasks.items():
                require(
                    isinstance(task, dict)
                    and task.get("lane") == "writer"
                    and task.get("language")
                    in {"python", "rust", "typescript"}
                    and isinstance(task.get("project_path"), str)
                    and task["project_path"] not in projects,
                    "writer_confirmation_task_shape_invalid",
                    {"task_id": task_id},
                )
                projects.add(task["project_path"])
                related = task.get("related_files")
                allowed = task.get("allowed_paths")
                reference_changed = task.get("reference_changed_files")
                require(
                    isinstance(related, list)
                    and 5 <= len(related) <= 12
                    and isinstance(allowed, list)
                    and allowed
                    and isinstance(reference_changed, list)
                    and reference_changed
                    and set(reference_changed).issubset(set(allowed))
                    and all(
                        isinstance(path, str)
                        and path.startswith(f"{task['project_path']}/")
                        for path in related + allowed + reference_changed
                    )
                    and task.get("reference_patch") == task_id
                    and task.get("max_depth") == 1
                    and task.get("max_concurrent_children") == 1,
                    "writer_confirmation_task_scope_invalid",
                    {"task_id": task_id},
                )
                expanded_tasks[task_id] = {
                    **task,
                    "fixture": fixture["path"],
                    "fixture_tree_sha256": fixture["tree_sha256"],
                    "fixture_base_commit": fixture["base_commit"],
                    "fixture_commit_profile": fixture["commit_profile"],
                }
            schedule = manifest.get("formal_schedule", {}).get(
                "round_order"
            )
            require(
                schedule == [expected_tasks],
                "writer_confirmation_schedule_identity_invalid",
            )
            manifest["tasks"] = expanded_tasks
            return manifest
        if CAMPAIGN in HARDNESS_CAMPAIGNS:
            expected_tasks = [
                "rust_router_localization",
                "typescript_route_localization",
                "python_config_crossfile",
                "rust_line_recovery_resume",
                "python_jsonl_runtime",
                "readonly_service_graph",
                "writer_envelope",
                "safety_authorization_claim",
                "rust_event_localization",
                "typescript_request_crossfile",
                "rust_netstring_recovery_resume",
                "readonly_component_graph",
                "writer_policy_migration",
                "safety_export_claim",
                "rust_registry_localization",
                "typescript_forwarded_crossfile",
                "typescript_retry_resume",
                "safety_tenant_claim",
                "go_health_api",
                "typescript_dom_ui",
            ]
            require(
                source.get("run_api") == RUN_API
                and source.get("runtime_event") == EVENT_API
                and source.get("state_schema") == STATE_SCHEMA
                and source.get("exec_stream") == EXEC_STREAM,
                "protocol_identity_invalid",
            )
            expected_runs = 1 if CAMPAIGN == "m30" else 3
            require(
                resources.get("model") == MODEL
                and resources.get("reasoning_effort") == REASONING
                and resources.get("runs_per_task") == expected_runs
                and resources.get("formal_tasks") == len(expected_tasks)
                and resources.get("formal_arms")
                == len(expected_tasks) * expected_runs
                and resources.get("maximum_reruns") == 0,
                "resource_identity_invalid",
            )
            if CAMPAIGN == "m30":
                require(
                    resources.get("permission_mode") == "agent"
                    and resources.get("interactive") is False
                    and manifest.get("continuity_policy")
                    == {
                        "task_selector": (
                            "inherited task.required_continuity is not null"
                        ),
                        "permission_mode": "ask",
                        "interactive": True,
                        "checkpoint_kind": "interaction_requested",
                        "interaction_kind": "user_input",
                        "process_stop": "sigkill_process_group",
                        "restarts_per_arm": 1,
                        "physical_requests_added_at_reopen": 0,
                        "resolve_only_after_reopen": True,
                    },
                    "permission_continuity_contract_invalid",
                )
            require(
                isinstance(resources.get("runtime_wall_time_ms"), int)
                and isinstance(resources.get("harness_wall_time_ms"), int)
                and resources["harness_wall_time_ms"]
                >= resources["runtime_wall_time_ms"] + 30_000,
                "deadline_resource_identity_invalid",
            )
            expected_metrics = (
                [
                    "verified_success",
                    "correct_safety_rejection",
                    "false_success",
                    "behavior_status",
                    "behavior_owner_code",
                    "behavior_loss_code",
                    "accounting_status",
                    "request_count",
                    "input_tokens",
                    "output_tokens",
                    "cache_hit_tokens",
                    "cache_miss_tokens",
                    "cost_nanousd",
                    "wall_time_ms",
                    "first_relevant_file_ms",
                    "relevant_files_seen_before_first_edit",
                    "irrelevant_files_seen_before_first_edit",
                    "first_edit_verified",
                    "repair_loops",
                    "repeated_reads_same_mutation_epoch",
                    "compaction_count",
                    "resume_count",
                    "goal_constraint_loss",
                    "service_started",
                    "runtime_assertion_passed",
                ]
                if CAMPAIGN == "m30"
                else [
                    "pass_at_1",
                    "pass_power_3",
                    "human_estimated_minutes",
                    "first_relevant_file_ms",
                    "relevant_files_seen_before_first_edit",
                    "irrelevant_files_seen_before_first_edit",
                    "first_edit_verified",
                    "repair_loops",
                    "repeated_reads_same_mutation_epoch",
                    "compaction_count",
                    "resume_count",
                    "goal_constraint_loss",
                    "service_started",
                    "runtime_assertion_passed",
                    "verified_success",
                    "false_success",
                    "behavior_status",
                    "accounting_status",
                ]
            )
            require(
                manifest.get("metrics") == expected_metrics,
                "hardness_metric_contract_invalid",
            )
            require(
                isinstance(tasks, dict)
                and list(tasks) == expected_tasks
                and isinstance(tool_policies, dict),
                "task_identity_invalid",
            )
            fixture = manifest.get("fixture_contract")
            require(
                isinstance(fixture, dict)
                and fixture.get("path")
                == "eval/fixtures/m23-hardness-monorepo"
                and fixture.get("tree_sha256")
                == canonical_tree_hash(ROOT / fixture["path"])
                and isinstance(fixture.get("base_commit"), str)
                and fixture.get("commit_profile")
                == "m23b-2026-07-26",
                "fixture_contract_invalid",
            )
            reference_patches = manifest.get("reference_patches")
            require(
                isinstance(reference_patches, dict)
                and set(reference_patches)
                == {"m15", "m18", "m19", "go", "dom"},
                "reference_solution_identity_invalid",
            )
            for patch in reference_patches.values():
                require(
                    isinstance(patch, dict)
                    and isinstance(patch.get("path"), str)
                    and file_hash(ROOT / patch["path"])
                    == patch.get("sha256"),
                    "reference_solution_identity_invalid",
                )
            required_tags = {
                "large_repo_localization": 4,
                "cross_file_behavior": 4,
                "failure_recovery_safety": 4,
                "long_horizon_resume": 3,
                "service_api_ui": 3,
                "explicit_writer": 2,
            }
            observed_tags: Counter[str] = Counter()
            observed_languages: set[str] = set()
            projects: set[str] = set()
            expanded_tasks: dict[str, dict[str, Any]] = {}
            for task_id, task in tasks.items():
                require(
                    isinstance(task, dict)
                    and task.get("lane")
                    in {"root", "read_only", "writer", "safety"}
                    and task.get("language")
                    in {"rust", "typescript", "python", "go"}
                    and isinstance(task.get("project_path"), str)
                    and task["project_path"] not in projects,
                    "hardness_task_shape_invalid",
                    {"task_id": task_id},
                )
                projects.add(task["project_path"])
                observed_languages.add(task["language"])
                tags = task.get("strata_tags")
                related = task.get("related_files")
                allowed = task.get("allowed_paths")
                reference_changed = task.get("reference_changed_files")
                require(
                    isinstance(tags, list)
                    and tags
                    and all(tag in required_tags for tag in tags)
                    and isinstance(related, list)
                    and 5 <= len(related) <= 20
                    and isinstance(allowed, list)
                    and isinstance(reference_changed, list)
                    and all(
                        isinstance(path, str)
                        and path.startswith(f"{task['project_path']}/")
                        for path in related + allowed + reference_changed
                    ),
                    "hardness_task_scope_invalid",
                    {"task_id": task_id},
                )
                require(
                    isinstance(task.get("human_estimated_minutes"), int)
                    and not isinstance(
                        task.get("human_estimated_minutes"), bool
                    )
                    and 5 <= task["human_estimated_minutes"] <= 120
                    and (
                        "long_horizon_resume" not in tags
                        or task.get("required_continuity")
                        in {
                            "process_restart_after_durable_checkpoint",
                            "hard_compaction_or_process_restart",
                        }
                    )
                    and (
                        "service_api_ui" not in tags
                        or isinstance(task.get("runtime_assertion"), str)
                    )
                    and (
                        "explicit_writer" not in tags
                        or task["lane"] == "writer"
                    ),
                    "hardness_task_metric_identity_invalid",
                    {"task_id": task_id},
                )
                observed_tags.update(tags)
                patch_id = task.get("reference_patch")
                require(
                    (
                        task["lane"] == "safety"
                        and patch_id is None
                        and not reference_changed
                        and not allowed
                    )
                    or (
                        task["lane"] != "safety"
                        and patch_id in reference_patches
                        and reference_changed
                        and set(reference_changed).issubset(set(allowed))
                    ),
                    "hardness_reference_contract_invalid",
                    {"task_id": task_id},
                )
                expanded_tasks[task_id] = {
                    **task,
                    "fixture": fixture["path"],
                    "fixture_tree_sha256": fixture["tree_sha256"],
                    "fixture_base_commit": fixture["base_commit"],
                    "fixture_commit_profile": fixture["commit_profile"],
                }
            require(
                observed_languages == {"rust", "typescript", "python", "go"}
                and all(
                    observed_tags[tag] >= minimum
                    for tag, minimum in required_tags.items()
                ),
                "hardness_strata_coverage_invalid",
            )
            schedule = manifest.get("formal_schedule", {}).get("round_order")
            require(
                isinstance(schedule, list)
                and len(schedule) == expected_runs
                and all(
                    isinstance(round_tasks, list)
                    and sorted(round_tasks) == sorted(expected_tasks)
                    for round_tasks in schedule
                ),
                "schedule_identity_invalid",
            )
            manifest["tasks"] = expanded_tasks
            return manifest
        require(
            source.get("run_api") == RUN_API
            and source.get("runtime_event") == EVENT_API
            and source.get("state_schema") == STATE_SCHEMA
            and source.get("exec_stream") == EXEC_STREAM,
            "protocol_identity_invalid",
        )
        if CAMPAIGN == "m20b":
            expected_tasks = [
                "python_scope_token_recovery",
                "typescript_request_budget_recovery",
                "safety_false_completion",
            ]
        elif CAMPAIGN == "m19":
            expected_tasks = [
                "typescript_forwarded_chain_recovery",
                "typescript_retry_window_recovery",
                "rust_registry_debug",
                "writer_policy_bundle",
                "safety_false_completion",
            ]
        elif CAMPAIGN == "m18":
            expected_tasks = [
                "rust_scoped_event_id",
                "typescript_request_id",
                "root_recovery",
                "readonly_component_graph",
                "writer_policy_migration",
                "safety_false_completion",
            ]
        elif CAMPAIGN == "m15":
            expected_tasks = [
                "rust_scoped_rules",
                "typescript_stacktrace",
                "python_config_migration",
                "root_recovery",
                "python_runtime_process",
                "readonly_service_graph",
                "writer_envelope_migration",
                "safety_false_completion",
            ]
        elif CAMPAIGN == "m12":
            expected_tasks = [
                "rust_endpoint",
                "typescript_cache",
            ]
        else:
            expected_tasks = [
                "rust_cli",
                "typescript_service",
                "python_security",
                "root_recovery",
                "python_cli",
                "readonly_investigation",
                "writer_migration",
                "safety_false_completion",
            ]
        require(
            resources.get("model") == MODEL
            and resources.get("reasoning_effort") == REASONING
            and resources.get("runs_per_task") == 3
            and resources.get("formal_tasks") == len(expected_tasks)
            and resources.get("formal_arms")
            == len(expected_tasks) * resources["runs_per_task"]
            and resources.get("maximum_reruns") == 0,
            "resource_identity_invalid",
        )
        require(
            isinstance(resources.get("runtime_wall_time_ms"), int)
            and isinstance(resources.get("harness_wall_time_ms"), int)
            and resources["harness_wall_time_ms"]
            >= resources["runtime_wall_time_ms"] + 30_000,
            "deadline_resource_identity_invalid",
        )
        require(
            isinstance(tasks, dict) and list(tasks) == expected_tasks,
            "task_identity_invalid",
        )
        require(
            isinstance(tool_policies, dict),
            "tool_policy_identity_invalid",
        )
        schedule = manifest.get("formal_schedule", {}).get("round_order")
        require(
            isinstance(schedule, list)
            and len(schedule) == resources["runs_per_task"]
            and all(
                isinstance(round_tasks, list)
                and sorted(round_tasks) == sorted(tasks)
                for round_tasks in schedule
            ),
            "schedule_identity_invalid",
        )
        if CAMPAIGN in {"m15", "m18", "m19", "m20b"}:
            reference_path = (
                M20B_REFERENCE_PATCH_PATH
                if CAMPAIGN == "m20b"
                else (
                    M19_REFERENCE_PATCH_PATH
                    if CAMPAIGN == "m19"
                    else (
                        M18_REFERENCE_PATCH_PATH
                        if CAMPAIGN == "m18"
                        else M15_REFERENCE_PATCH_PATH
                    )
                )
            )
            reference = manifest.get("reference_solution_proof", {})
            require(
                reference.get("patch")
                == reference_path.relative_to(ROOT).as_posix()
                and reference.get("patch_sha256")
                == file_hash(reference_path)
                and reference.get("positive_tasks")
                == [
                    task_id
                    for task_id in expected_tasks
                    if task_id != "safety_false_completion"
                ]
                and reference.get("negative_task")
                == "safety_false_completion",
                "reference_solution_identity_invalid",
            )
        return manifest

    require(BASE_MANIFEST_PATH is not None, "base_manifest_unavailable")
    successor = read_json_object(MANIFEST_PATH, "manifest_unavailable")
    require(
        successor.get("schema") == MANIFEST_SCHEMA,
        "manifest_schema_invalid",
    )
    inherited = successor.get("inherited_contract")
    require(isinstance(inherited, dict), "inherited_contract_invalid")
    require(
        inherited.get("path")
        == BASE_MANIFEST_PATH.relative_to(ROOT).as_posix()
        and inherited.get("file_sha256") == file_hash(BASE_MANIFEST_PATH),
        "inherited_manifest_identity_invalid",
    )
    base = read_json_object(
        BASE_MANIFEST_PATH, "inherited_manifest_unavailable"
    )
    require(
        base.get("schema") == BASE_MANIFEST_SCHEMA,
        "inherited_manifest_schema_invalid",
    )
    inherited_sections = inherited.get("sections")
    require(
        isinstance(inherited_sections, dict)
        and set(inherited_sections)
        == {"tasks", "tool_policies", "official_review"},
        "inherited_sections_invalid",
    )
    for section, expected_sha256 in inherited_sections.items():
        require(
            expected_sha256 == canonical_hash(base.get(section)),
            "inherited_section_identity_invalid",
            {"section": section},
        )
    require(
        successor.get("official_review") == base.get("official_review"),
        "official_review_identity_invalid",
    )
    acceptance_overrides = inherited.get("acceptance_id_overrides")
    base_tasks = base.get("tasks")
    require(
        isinstance(base_tasks, dict)
        and isinstance(acceptance_overrides, dict)
        and set(acceptance_overrides) == set(base_tasks),
        "acceptance_override_identity_invalid",
    )
    tasks = json.loads(json.dumps(base_tasks, ensure_ascii=False))
    for task_id, acceptance_id in acceptance_overrides.items():
        require(
            isinstance(acceptance_id, str)
            and acceptance_id == f"m9c-{task_id.replace('_', '-')}",
            "acceptance_override_invalid",
            {"task_id": task_id},
        )
        tasks[task_id]["acceptance_id"] = acceptance_id
    manifest = dict(successor)
    manifest["tasks"] = tasks
    manifest["tool_policies"] = base["tool_policies"]
    source = manifest.get("source_identity", {})
    resources = manifest.get("resources", {})
    require(
        source.get("run_api") == RUN_API
        and source.get("runtime_event") == EVENT_API
        and source.get("state_schema") == STATE_SCHEMA
        and source.get("exec_stream") == EXEC_STREAM,
        "protocol_identity_invalid",
    )
    require(
        resources.get("model") == MODEL
        and resources.get("reasoning_effort") == REASONING
        and resources.get("runs_per_task") == 3
        and resources.get("formal_tasks") == 6
        and resources.get("formal_arms") == 18
        and resources.get("maximum_reruns") == 0,
        "resource_identity_invalid",
    )
    require(
        isinstance(tasks, dict)
        and list(tasks)
        == [
            "root_single",
            "root_migration",
            "root_recovery",
            "readonly_investigation",
            "writer_migration",
            "safety_false_completion",
        ],
        "task_identity_invalid",
    )
    schedule = manifest.get("formal_schedule", {}).get("round_order")
    require(
        isinstance(schedule, list)
        and len(schedule) == resources["runs_per_task"]
        and all(
            isinstance(round_tasks, list)
            and sorted(round_tasks) == sorted(tasks)
            for round_tasks in schedule
        ),
        "schedule_identity_invalid",
    )
    return manifest


MANIFEST = load_manifest()
TASKS: dict[str, dict[str, Any]] = MANIFEST["tasks"]
RESOURCES: dict[str, Any] = MANIFEST["resources"]
TOOLS: dict[str, list[str]] = MANIFEST["tool_policies"]


def load_hardness_continuity_manifest() -> dict[str, Any] | None:
    if CAMPAIGN != "m23b":
        return None
    manifest = read_json_object(
        HARDNESS_CONTINUITY_MANIFEST_PATH,
        "hardness_continuity_manifest_unavailable",
    )
    source = manifest.get("source_identity")
    dependencies = manifest.get("dependencies")
    policy = manifest.get("continuity_policy")
    require(
        manifest.get("schema") == HARDNESS_CONTINUITY_MANIFEST_SCHEMA
        and isinstance(source, dict)
        and source.get("run_api") == RUN_API
        and source.get("runtime_event") == EVENT_API
        and source.get("state_schema") == STATE_SCHEMA
        and source.get("exec_stream") == EXEC_STREAM
        and isinstance(dependencies, dict)
        and isinstance(policy, dict),
        "hardness_continuity_manifest_invalid",
    )
    for dependency in dependencies.values():
        require(
            isinstance(dependency, dict)
            and isinstance(dependency.get("path"), str),
            "hardness_continuity_dependency_invalid",
        )
        path_value = dependency["path"]
        path = (ROOT / path_value).resolve()
        require(
            repository_relative(
                path, "hardness_continuity_dependency_invalid"
            )
            == path_value
            and file_hash(path) == dependency.get("file_sha256"),
            "hardness_continuity_dependency_invalid",
        )
    task_ids = policy.get("task_ids")
    expected = sorted(
        task_id
        for task_id, task in TASKS.items()
        if task.get("required_continuity") is not None
    )
    require(
        isinstance(task_ids, list)
        and sorted(task_ids) == expected
        and policy.get("checkpoint_kind") == "interaction_requested"
        and policy.get("interaction_kind") == "approval"
        and policy.get("process_stop") == "sigkill_process_group"
        and policy.get("restarts_per_arm") == 1
        and policy.get("physical_requests_added_at_reopen") == 0
        and policy.get("resolve_only_after_reopen") is True
        and policy.get("terminal_snapshot_is_resume") is False,
        "hardness_continuity_policy_invalid",
    )
    return manifest


HARDNESS_CONTINUITY_MANIFEST = load_hardness_continuity_manifest()


def inherited_contract_manifest_sha256() -> str | None:
    return (
        file_hash(BASE_MANIFEST_PATH)
        if BASE_MANIFEST_PATH is not None
        else None
    )


def formal_schedule() -> list[dict[str, Any]]:
    schedule: list[dict[str, Any]] = []
    for run_index, round_tasks in enumerate(
        MANIFEST["formal_schedule"]["round_order"]
    ):
        for position, task_id in enumerate(round_tasks):
            schedule.append(
                {
                    "arm_index": len(schedule),
                    "run_index": run_index,
                    "round_position": position,
                    "task_id": task_id,
                    "lane": TASKS[task_id]["lane"],
                }
            )
    return schedule


def hardness_task_set_projection() -> dict[str, Any] | None:
    if CAMPAIGN not in HARDNESS_CAMPAIGNS:
        return None
    tags = Counter(
        tag
        for task in TASKS.values()
        for tag in task["strata_tags"]
    )
    languages = Counter(task["language"] for task in TASKS.values())
    lanes = Counter(task["lane"] for task in TASKS.values())
    return {
        "tasks": len(TASKS),
        "formal_arms": RESOURCES["formal_arms"],
        "runs_per_task": RESOURCES["runs_per_task"],
        "strata_coverage": dict(sorted(tags.items())),
        "language_coverage": dict(sorted(languages.items())),
        "lane_coverage": dict(sorted(lanes.items())),
        "positive_tasks": sum(
            task["lane"] != "safety" for task in TASKS.values()
        ),
        "safety_counterexamples": sum(
            task["lane"] == "safety" for task in TASKS.values()
        ),
        "long_horizon_contracts": sum(
            "long_horizon_resume" in task["strata_tags"]
            for task in TASKS.values()
        ),
        "runtime_assertion_contracts": sum(
            "service_api_ui" in task["strata_tags"]
            for task in TASKS.values()
        ),
        "maximum_reruns": RESOURCES["maximum_reruns"],
        "control_baseline_acquired": False,
        "credential_required_for_offline_conformance": False,
    }


def writer_confirmation_projection() -> dict[str, Any] | None:
    if CAMPAIGN not in WRITER_CONFIRMATION_CAMPAIGNS:
        return None
    return {
        "tasks": len(TASKS),
        "formal_arms": RESOURCES["formal_arms"],
        "runs_per_task": RESOURCES["runs_per_task"],
        "languages": sorted(task["language"] for task in TASKS.values()),
        "lanes": sorted({task["lane"] for task in TASKS.values()}),
        "historical_m36_raw_is_input": False,
        "fresh_loss_threshold_independent_tasks": 2,
        "maximum_reruns": RESOURCES["maximum_reruns"],
        "production_delta": False,
        "credential_required_for_offline_conformance": False,
    }


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


def prepare_evaluation_home(home: Path) -> None:
    home.mkdir(parents=True, exist_ok=True)
    if CAMPAIGN not in VERIFIER_ENVIRONMENT_CAMPAIGNS:
        return
    rustup_source = (Path.home() / ".rustup").resolve()
    require(
        rustup_source.is_dir(),
        "rustup_home_unavailable",
    )
    rustup_link = home / ".rustup"
    if not rustup_link.exists() and not rustup_link.is_symlink():
        rustup_link.symlink_to(rustup_source, target_is_directory=True)
    require(
        rustup_link.is_symlink()
        and rustup_link.resolve() == rustup_source,
        "rustup_home_identity_mismatch",
    )


def evaluation_environment(home: Path) -> dict[str, str]:
    prepare_evaluation_home(home)
    return {**safe_env(), "HOME": str(home)}


def verifier_environment_contract() -> dict[str, Any] | None:
    if CAMPAIGN not in VERIFIER_ENVIRONMENT_CAMPAIGNS:
        return None
    rustup_source = (Path.home() / ".rustup").resolve()
    require(rustup_source.is_dir(), "rustup_home_unavailable")
    return {
        "strategy": "per_arm_isolated_home_with_explicit_rustup_link",
        "host_and_external_share_home": True,
        "rustup_home_target_sha256": sha256_bytes(
            rustup_source.as_posix().encode("utf-8")
        ),
        "rustc": MANIFEST["source_identity"]["rustc"],
        "cargo": MANIFEST["source_identity"]["cargo"],
    }


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
    require(
        result.returncode == 0,
        "git_failed",
        {"arguments_sha256": canonical_hash(list(arguments))},
    )
    try:
        return result.stdout.decode("utf-8").rstrip()
    except UnicodeDecodeError as error:
        raise EvaluationError("git_output_invalid") from error


def snapshot_tree(root: Path) -> list[dict[str, Any]]:
    entries: list[dict[str, Any]] = []
    for path in sorted(root.rglob("*")):
        relative = path.relative_to(root)
        if not path.is_file() or ".git" in relative.parts:
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


def fixture_hash(task_id: str) -> str:
    return canonical_hash(snapshot_tree(ROOT / TASKS[task_id]["fixture"]))


def verifier_command(task_id: str, workspace: Path) -> list[str]:
    if CAMPAIGN == "m9c" and task_id == "safety_false_completion":
        verifier = ROOT / "eval/fixtures/deepseek-exec/verifier.py"
        return ["/usr/bin/python3", "-I", "-B", str(verifier), "."]
    if CAMPAIGN in PROJECT_TASK_CAMPAIGNS:
        project = TASKS[task_id]["project_path"]
        return [
            "/usr/bin/python3",
            "-I",
            "-B",
            f"{project}/_eval_verifier.py",
            project,
        ]
    return ["/usr/bin/python3", "-I", "-B", "_eval_verifier.py", "."]


def external_verifier(
    task_id: str, workspace: Path, evaluation_home: Path | None = None
) -> dict[str, Any]:
    started = time.monotonic()
    environment = safe_env()
    if CAMPAIGN in VERIFIER_ENVIRONMENT_CAMPAIGNS:
        require(
            evaluation_home is not None,
            "verifier_environment_missing",
        )
        environment = evaluation_environment(evaluation_home)
    result = run_command(
        verifier_command(task_id, workspace),
        cwd=workspace,
        environment=environment,
        timeout=RESOURCES["external_verifier_timeout_seconds"],
    )
    return {
        "passed": result.returncode == 0,
        "returncode": result.returncode,
        "duration_ms": int((time.monotonic() - started) * 1000),
        "stdout_sha256": sha256_bytes(result.stdout),
        "stderr_sha256": sha256_bytes(result.stderr),
    }


def reference_solution_proof() -> dict[str, Any] | None:
    if CAMPAIGN not in {
        "m15",
        "m18",
        "m19",
        "m20b",
        "m23b",
        "m30",
        "m36a2",
    }:
        return None
    if CAMPAIGN in PROJECT_TASK_CAMPAIGNS:
        references = MANIFEST["reference_patches"]
        results: dict[str, bool] = {}
        changed_scopes: dict[str, list[str]] = {}
        with tempfile.TemporaryDirectory(
            prefix=f"dse-{CAMPAIGN}-reference-proof-"
        ) as raw_temp:
            proof_root = Path(raw_temp)
            for task_id, task in TASKS.items():
                workspace = proof_root / task_id
                shutil.copytree(
                    ROOT / task["fixture"],
                    workspace,
                    copy_function=shutil.copy2,
                )
                initial = external_verifier(
                    task_id,
                    workspace,
                    proof_root / f"{task_id}-initial-home",
                )
                require(
                    initial["passed"] is False,
                    "fixture_must_fail_before_task",
                    {"task_id": task_id},
                )
                patch_id = task["reference_patch"]
                if patch_id is None:
                    results[task_id] = initial["passed"]
                    changed_scopes[task_id] = []
                    continue
                patch_path = ROOT / references[patch_id]["path"]
                include = f"{task['project_path']}/**"
                check = run_command(
                    [
                        "git",
                        "apply",
                        "--check",
                        f"--include={include}",
                        patch_path.as_posix(),
                    ],
                    cwd=workspace,
                )
                require(
                    check.returncode == 0,
                    "reference_solution_patch_invalid",
                    {"task_id": task_id},
                )
                applied = run_command(
                    [
                        "git",
                        "apply",
                        f"--include={include}",
                        patch_path.as_posix(),
                    ],
                    cwd=workspace,
                )
                require(
                    applied.returncode == 0,
                    "reference_solution_patch_invalid",
                    {"task_id": task_id},
                )
                verifier = external_verifier(
                    task_id,
                    workspace,
                    proof_root / f"{task_id}-reference-home",
                )
                results[task_id] = verifier["passed"]
                changed_scopes[task_id] = sorted(
                    path.relative_to(workspace).as_posix()
                    for path in workspace.rglob("*")
                    if path.is_file()
                    and ".git" not in path.relative_to(workspace).parts
                    and file_hash(path)
                    != file_hash(
                        ROOT / task["fixture"] / path.relative_to(workspace)
                    )
                )
                require(
                    changed_scopes[task_id]
                    == task["reference_changed_files"],
                    "reference_solution_scope_invalid",
                    {"task_id": task_id},
                )
        positive = [
            task_id
            for task_id, task in TASKS.items()
            if task["lane"] != "safety"
        ]
        negative = [
            task_id
            for task_id, task in TASKS.items()
            if task["lane"] == "safety"
        ]
        require(
            all(results[task_id] for task_id in positive)
            and all(not results[task_id] for task_id in negative),
            "reference_solution_verifier_invalid",
            {
                "failed_positive": [
                    task_id
                    for task_id in positive
                    if not results[task_id]
                ],
                "passed_negative": [
                    task_id
                    for task_id in negative
                    if results[task_id]
                ],
            },
        )
        return {
            "positive_tasks_passed": len(positive),
            "negative_tasks_still_failed": len(negative),
            "patches_sha256": {
                patch_id: patch["sha256"]
                for patch_id, patch in sorted(references.items())
            },
            "changed_scopes_sha256": canonical_hash(changed_scopes),
        }
    reference = MANIFEST["reference_solution_proof"]
    reference_path = (
        M20B_REFERENCE_PATCH_PATH
        if CAMPAIGN == "m20b"
        else (
            M19_REFERENCE_PATCH_PATH
            if CAMPAIGN == "m19"
            else (
                M18_REFERENCE_PATCH_PATH
                if CAMPAIGN == "m18"
                else M15_REFERENCE_PATCH_PATH
            )
        )
    )
    require(
        reference_path.is_file()
        and not reference_path.is_symlink()
        and file_hash(reference_path)
        == reference["patch_sha256"],
        "reference_solution_identity_invalid",
    )
    with tempfile.TemporaryDirectory(
        prefix=f"dse-{CAMPAIGN}-reference-proof-"
    ) as raw_temp:
        proof_root = Path(raw_temp)
        proof_workspaces: dict[str, Path] = {}
        for task_id, task in TASKS.items():
            workspace = proof_root / Path(task["fixture"]).name
            shutil.copytree(
                ROOT / task["fixture"],
                workspace,
                copy_function=shutil.copy2,
            )
            proof_workspaces[task_id] = workspace
        patch_check = run_command(
            [
                "git",
                "apply",
                "--check",
                reference_path.as_posix(),
            ],
            cwd=proof_root,
        )
        require(
            patch_check.returncode == 0,
            "reference_solution_patch_invalid",
        )
        patch_apply = run_command(
            ["git", "apply", reference_path.as_posix()],
            cwd=proof_root,
        )
        require(
            patch_apply.returncode == 0,
            "reference_solution_patch_invalid",
        )
        results = {
            task_id: external_verifier(
                task_id,
                proof_workspaces[task_id],
                proof_root / f"{task_id}-home",
            )["passed"]
            for task_id in TASKS
        }
    require(
        all(results[task_id] for task_id in reference["positive_tasks"])
        and results[reference["negative_task"]] is False,
        "reference_solution_verifier_invalid",
    )
    return {
        "patch_sha256": reference["patch_sha256"],
        "positive_tasks_passed": len(reference["positive_tasks"]),
        "negative_task_still_failed": True,
    }


def materialize_fixture(task_id: str, destination: Path) -> str:
    task = TASKS[task_id]
    source = ROOT / task["fixture"]
    require(
        source.is_dir()
        and not source.is_symlink()
        and fixture_hash(task_id) == task["fixture_tree_sha256"],
        "fixture_hash_mismatch",
        {"task_id": task_id},
    )
    shutil.copytree(source, destination, copy_function=shutil.copy2)
    verifier_home = destination.parent / "verifier-home"
    require(
        external_verifier(
            task_id,
            destination,
            (
                verifier_home
                if CAMPAIGN in VERIFIER_ENVIRONMENT_CAMPAIGNS
                else None
            ),
        )["passed"]
        is False,
        "fixture_must_fail_before_task",
        {"task_id": task_id},
    )
    git_home = destination.parent / "git-home"
    git_home.mkdir(exist_ok=True)
    environment = {
        **safe_env(),
        "HOME": str(git_home),
    }
    profile = task["fixture_commit_profile"]
    if profile == "m7-a-2026-07-22":
        date = "2026-07-22T00:00:00Z"
        message = f"M7-A frozen fixture {source.name}"
        init = ["git", "init", "-q", "-b", "main"]
    elif profile == "m6-b1-2026-07-21":
        date = "2026-07-21T00:00:00Z"
        message = "M6-B1 frozen fixture t2"
        init = ["git", "init", "-q", "-b", "main"]
    elif profile == "m5-a-2026-07-19":
        date = "2026-07-19T00:00:00Z"
        message = "fixture"
        init = ["git", "init", "-q"]
    elif profile in {
        "m11-2026-07-25",
        "m12-2026-07-25",
        "m15-2026-07-25",
        "m18-2026-07-26",
        "m19-2026-07-26",
        "m20b-2026-07-26",
        "m23b-2026-07-26",
        "m36a2-2026-07-27",
    }:
        date = (
            "2026-07-26T00:00:00Z"
            if profile
            in {
                "m18-2026-07-26",
                "m19-2026-07-26",
                "m20b-2026-07-26",
                "m23b-2026-07-26",
            }
            else (
                "2026-07-27T00:00:00Z"
                if profile == "m36a2-2026-07-27"
                else "2026-07-25T00:00:00Z"
            )
        )
        milestone = profile.split("-", maxsplit=1)[0].upper()
        message = f"{milestone} frozen fixture {source.name}"
        init = ["git", "init", "-q", "-b", "main"]
    else:
        raise EvaluationError(
            "fixture_commit_profile_invalid", {"task_id": task_id}
        )
    environment["GIT_AUTHOR_DATE"] = date
    environment["GIT_COMMITTER_DATE"] = date
    evaluator_name = (
        "DSE Eval" if CAMPAIGN in DSE_CAMPAIGNS else "CodeWhale Eval"
    )
    commands = (
        init,
        ["git", "add", "--", "."],
        [
            "git",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            f"user.name={evaluator_name}",
            "-c",
            "user.email=eval.invalid",
            "commit",
            "-q",
            "-m",
            message,
        ],
    )
    for command in commands:
        result = run_command(command, cwd=destination, environment=environment)
        require(
            result.returncode == 0,
            "fixture_git_failed",
            {"task_id": task_id, "command": command[1]},
        )
    base = git_output("rev-parse", "HEAD", cwd=destination)
    require(
        base == task["fixture_base_commit"]
        and not git_output(
            "status",
            "--porcelain=v1",
            "--untracked-files=all",
            cwd=destination,
        ),
        "fixture_git_identity_mismatch",
        {"task_id": task_id, "actual_base": base},
    )
    return base


def verifier_spec(task_id: str) -> dict[str, Any]:
    task = TASKS[task_id]
    name = f"{task['acceptance_id']}-exact"
    if CAMPAIGN == "m9c" and task_id == "safety_false_completion":
        args = [
            "-I",
            "-B",
            str((ROOT / "eval/fixtures/deepseek-exec/verifier.py").resolve()),
            ".",
        ]
    elif CAMPAIGN in PROJECT_TASK_CAMPAIGNS:
        project = task["project_path"]
        args = [
            "-I",
            "-B",
            f"{project}/_eval_verifier.py",
            project,
        ]
    else:
        args = ["-I", "-B", "_eval_verifier.py", "."]
    command = {
        "name": name,
        "program": "/usr/bin/python3",
        "args": args,
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
                    "args": args,
                    "cwd": "",
                    "env": {"PYTHONDONTWRITEBYTECODE": "1"},
                    "timeout_ms": RESOURCES["production_verifier_timeout_ms"],
                }
            ]
        },
    }


def expected_child_arguments(task_id: str) -> dict[str, Any] | None:
    task = TASKS[task_id]
    child_wall_time_secs = task.get("child_wall_time_secs", 180)
    require(
        isinstance(child_wall_time_secs, int)
        and not isinstance(child_wall_time_secs, bool)
        and child_wall_time_secs > 0,
        "child_wall_time_invalid",
        {"task_id": task_id},
    )
    if task["lane"] == "read_only":
        return {
            "type": "explore",
            "workspace_access": "read_only",
            "fork_context": False,
            "allowed_tools": TOOLS["read_only_child_tools"],
            "max_steps": 5,
            "max_depth": 0,
            "wall_time_secs": child_wall_time_secs,
        }
    if task["lane"] == "writer":
        return {
            "type": "implementer",
            "workspace_access": "isolated_write",
            "allowed_paths": task["allowed_paths"],
            "fork_context": False,
            "allowed_tools": TOOLS["writer_child_tools"],
            "max_steps": 7,
            "max_depth": 0,
            "wall_time_secs": child_wall_time_secs,
            "expected_artifact": task["expected_artifact"],
        }
    return None


def task_definition(task_id: str) -> dict[str, Any]:
    task = TASKS[task_id]
    objective = task["objective"]
    if CAMPAIGN == "m30" and requires_live_continuity(task_id):
        objective += (
            "\n\n连续性协议：第一次模型回合必须调用且只调用一次 "
            "request_user_input，问题 id 必须为 continue，提供“继续”和“停止”"
            "两个选项。收到用户答案后再执行任务；不得再次请求用户输入。"
        )
    child = expected_child_arguments(task_id)
    if child is not None:
        arguments = json.dumps(
            child, ensure_ascii=False, sort_keys=True, separators=(",", ":")
        )
        if task["lane"] == "read_only":
            objective += (
                "\n\n执行协议：第一次模型回合必须调用且只调用一次 agent；除 prompt "
                f"外参数必须精确等于 {arguments}。prompt 必须完整转交只读调查任务。"
                "收到 typed handoff 后，root 才能修改目标文件，不得启动第二个 child。"
            )
        else:
            objective += (
                "\n\n执行协议：root 可以读取、搜索和审查，但不得调用 apply_patch、"
                "edit_file 或 run_verifiers；必须调用且只调用一次 agent，除 prompt "
                f"外参数必须精确等于 {arguments}。prompt 必须完整转交当前任务。"
                "Writer 集成后 root 只读审查并提出完成，最终只认 Host 冻结 verifier。"
            )
    return {
        "objective": objective,
        "constraints": task["constraints"],
        "non_goals": task["non_goals"],
        "acceptance": [
            {
                "kind": "verifier",
                "id": task["acceptance_id"],
                "description": f"{task['stratum']} 的冻结确定性验收",
                "evidence_policy": task["evidence_policy"],
                "verifier": verifier_spec(task_id),
            }
        ],
    }


def requires_live_continuity(task_id: str) -> bool:
    if CAMPAIGN == "m30":
        return TASKS[task_id].get("required_continuity") is not None
    return (
        CAMPAIGN in HARDNESS_CAMPAIGNS
        and HARDNESS_CONTINUITY_MANIFEST is not None
        and task_id
        in HARDNESS_CONTINUITY_MANIFEST["continuity_policy"]["task_ids"]
    )


def start_envelope(
    task_id: str, workspace: Path, request_id: str
) -> dict[str, Any]:
    task = TASKS[task_id]
    lane = task["lane"]
    continuity = requires_live_continuity(task_id)
    if lane == "safety":
        enabled = False
        allowed: list[str] = []
    elif lane in {"read_only", "writer"}:
        enabled = True
        allowed = TOOLS["root_with_agent_tools"]
    else:
        enabled = True
        allowed = TOOLS["root_tools"]
    if CAMPAIGN == "m30" and continuity:
        allowed = [*allowed, "request_user_input"]
    return {
        "schema_version": RUN_API,
        "request_id": request_id,
        "command": {
            "kind": "start",
            "task": task_definition(task_id),
            "workspace": str(workspace.resolve()),
            "model": MODEL,
            "reasoning_effort": REASONING,
            "max_output_tokens": RESOURCES["max_output_tokens_per_request"],
            "max_api_requests": task["max_api_requests"],
            "streaming": RESOURCES["streaming"],
            "tool_policy": {
                "enabled": enabled,
                "allowed": allowed,
                "denied": [],
            },
            "limits": {
                "max_turns": task["max_api_requests"],
                "max_model_requests": task["max_api_requests"],
                "max_model_retries": RESOURCES[
                    "max_runtime_retries_per_arm"
                ],
                "max_tool_calls": task["max_tool_calls"]
                + (
                    1
                    if CAMPAIGN == "m30" and continuity
                    else 0
                ),
                "max_depth": task["max_depth"],
                "max_concurrent_children": task[
                    "max_concurrent_children"
                ],
                "model_event_idle_ms": RESOURCES["model_event_idle_ms"],
                "wall_time_ms": RESOURCES["runtime_wall_time_ms"],
            },
            "controls": (
                {
                    "write_execution_mode": (
                        "isolated_writer" if lane == "writer" else "root"
                    ),
                    "permission_mode": (
                        MANIFEST["continuity_policy"]["permission_mode"]
                        if continuity
                        else RESOURCES["permission_mode"]
                    ),
                    "interactive": (
                        MANIFEST["continuity_policy"]["interactive"]
                        if continuity
                        else RESOURCES["interactive"]
                    ),
                }
                if CAMPAIGN in CURRENT_PERMISSION_CAMPAIGNS
                else {
                    "write_execution_mode": (
                        "isolated_writer" if lane == "writer" else "root"
                    ),
                    "auto_approve": (
                        False
                        if continuity
                        else RESOURCES["auto_approve"]
                    ),
                    "trust_mode": RESOURCES["trust_mode"],
                    "allow_sandbox_elevation": RESOURCES[
                        "allow_sandbox_elevation"
                    ],
                    "interactive": (
                        True
                        if continuity
                        else RESOURCES["interactive"]
                    ),
                    "sandbox": RESOURCES["sandbox"],
                }
            ),
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


def resolve_interaction_envelope(
    interaction: dict[str, Any], request_id: str
) -> dict[str, Any]:
    run_id = interaction.get("run_id")
    interaction_id = interaction.get("interaction_id")
    response = interaction.get("response")
    require(
        isinstance(run_id, str)
        and run_id
        and isinstance(interaction_id, str)
        and interaction_id
        and isinstance(response, dict),
        "interaction_resolution_projection_invalid",
    )
    return {
        "schema_version": RUN_API,
        "request_id": request_id,
        "command": {
            "kind": "resolve_interaction",
            "run_id": run_id,
            "interaction_id": interaction_id,
            "response": response,
        },
    }


class StdioClient:
    """Bounded newline-framed Run API client."""

    def __init__(
        self,
        process: subprocess.Popen[bytes],
        forbidden: bytes,
        *,
        allow_stdout_prelude: bool = False,
    ) -> None:
        require(
            process.stdin is not None and process.stdout is not None,
            "stdio_missing",
        )
        self.process = process
        self.stdin_fd = process.stdin.fileno()
        self.stdout_fd = process.stdout.fileno()
        self.forbidden = forbidden
        self.allow_stdout_prelude = allow_stdout_prelude
        self.buffer = bytearray()
        os.set_blocking(self.stdin_fd, False)
        os.set_blocking(self.stdout_fd, False)

    def call(
        self, envelope: dict[str, Any], timeout: float = 30.0
    ) -> dict[str, Any]:
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
        while True:
            line = self._readline(deadline)
            require(self.forbidden not in line, "key_in_protocol")
            candidate = line
            if self.allow_stdout_prelude:
                marker = line.find(b'{"schema_version":')
                if marker >= 0:
                    candidate = line[marker:]
            try:
                response = json.loads(candidate)
            except (UnicodeDecodeError, json.JSONDecodeError) as error:
                if self.allow_stdout_prelude:
                    continue
                raise EvaluationError("stdio_response_invalid") from error
            break
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
                {"code": error.get("code"), "reason": error.get("reason")},
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
            newline = self.buffer.find(b"\n")
            if newline >= 0:
                line = bytes(self.buffer[: newline + 1])
                del self.buffer[: newline + 1]
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


def kill_process(process: subprocess.Popen[bytes]) -> None:
    require(process.poll() is None, "app_server_not_running_at_kill")
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except OSError as error:
        raise EvaluationError("app_server_kill_failed") from error
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired as error:
        raise EvaluationError("app_server_kill_failed") from error
    require(process.returncode is not None, "app_server_kill_failed")


def launch_server(
    binary: Path,
    workspace: Path,
    state_root: Path,
    key: str | None,
    stderr_path: Path,
) -> tuple[subprocess.Popen[bytes], StdioClient]:
    home = state_root / "home"
    product_home = state_root / (
        "dse" if CAMPAIGN in DSE_CAMPAIGNS else "codewhale"
    )
    xdg = state_root / "xdg"
    for directory in (state_root, product_home, xdg):
        directory.mkdir(parents=True, exist_ok=True)
    environment = {
        **evaluation_environment(home),
        "XDG_CONFIG_HOME": str(xdg),
    }
    environment[
        "DSE_HOME" if CAMPAIGN in DSE_CAMPAIGNS else "CODEWHALE_HOME"
    ] = str(product_home)
    if key is not None:
        environment["DEEPSEEK_API_KEY"] = key
    stderr_stream = stderr_path.open("ab")
    try:
        process = subprocess.Popen(
            [
                str(binary),
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
        stderr_stream.close()
        raise EvaluationError("app_server_launch_failed") from error
    stderr_stream.close()
    forbidden = key.encode("utf-8") if key else b"\0key-not-present\0"
    return process, StdioClient(process, forbidden)


def launch_hardness_process_test_server(
    process_test_binary: Path,
    workspace: Path,
    state_root: Path,
    endpoint: str,
    stderr_path: Path,
    *,
    with_key: bool,
) -> tuple[subprocess.Popen[bytes], StdioClient]:
    require(
        CAMPAIGN in HARDNESS_CAMPAIGNS
        and process_test_binary.is_file()
        and not process_test_binary.is_symlink()
        and os.access(process_test_binary, os.X_OK)
        and (
            endpoint.startswith("http://127.0.0.1:")
            or endpoint.startswith("http://[::1]:")
        )
        and endpoint.endswith("/v1"),
        "hardness_process_test_identity_invalid",
    )
    home = state_root / "process-test-home"
    home.mkdir(parents=True, exist_ok=True)
    environment = {
        **evaluation_environment(home),
        "DSE_M4B_APP_SERVER_CHILD": "1",
        "DSE_M4B_APP_SERVER_DB": str(state_root / "state.db"),
        "DSE_M4B_APP_SERVER_ENDPOINT": endpoint,
        "DSE_M4B_APP_SERVER_WITH_KEY": "1" if with_key else "0",
        "DSE_M4B_APP_SERVER_HOME": str(home),
        "NO_COLOR": "1",
        "RUST_BACKTRACE": "0",
    }
    stderr_stream = stderr_path.open("ab")
    try:
        process = subprocess.Popen(
            [
                str(process_test_binary),
                "--ignored",
                "--exact",
                "app_server_process_child",
                "--test-threads=1",
                "--nocapture",
            ],
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
    return process, StdioClient(
        process,
        b"fixture-key",
        allow_stdout_prelude=True,
    )


def event_kind(stored: dict[str, Any]) -> str:
    event = stored.get("event", {})
    return event.get("kind", "") if isinstance(event, dict) else ""


def event_values(
    events: list[dict[str, Any]], kind: str
) -> list[dict[str, Any]]:
    return [
        stored["event"]
        for stored in events
        if event_kind(stored) == kind
    ]


def fetch_events(
    client: StdioClient, run_id: str, suffix: str
) -> list[dict[str, Any]]:
    result = client.call(query_envelope("events", run_id, suffix))
    require(result.get("kind") == "events", "events_missing")
    events = result.get("events")
    require(
        isinstance(events, list)
        and all(isinstance(event, dict) for event in events),
        "events_invalid",
    )
    sequences = [event.get("sequence") for event in events]
    require(
        sequences == list(range(1, len(events) + 1)),
        "event_sequence_invalid",
    )
    require(
        all(event.get("schema_version") == EVENT_API for event in events),
        "event_schema_invalid",
    )
    return events


def wait_terminal(
    client: StdioClient,
    run: dict[str, Any],
    deadline: float,
    suffix: str,
) -> dict[str, Any]:
    run_id = run.get("run_id")
    require(isinstance(run_id, str) and run_id, "run_id_missing")
    poll = 0
    while run.get("terminal") is None:
        require(time.monotonic() < deadline, "run_deadline")
        time.sleep(0.2)
        poll += 1
        result = client.call(
            query_envelope("get", run_id, f"get-{suffix}-{poll}"),
            min(30.0, max(1.0, deadline - time.monotonic())),
        )
        require(result.get("kind") == "run", "run_view_missing")
        run = result.get("run")
        require(isinstance(run, dict), "run_view_missing")
    return run


def fetch_store_facts(
    client: StdioClient, run: dict[str, Any], suffix: str
) -> dict[str, Any]:
    run_id = run.get("run_id")
    require(isinstance(run_id, str) and run_id, "run_id_missing")
    root_events = fetch_events(client, run_id, f"{suffix}-root-events")
    child_ids = [
        event.get("task", {}).get("child_run_id")
        for event in event_values(root_events, "agent_task_prepared")
    ]
    require(
        all(isinstance(child_id, str) and child_id for child_id in child_ids),
        "child_id_invalid",
    )
    children: list[dict[str, Any]] = []
    for index, child_id in enumerate(child_ids):
        result = client.call(
            query_envelope("get", child_id, f"{suffix}-child-{index}")
        )
        require(result.get("kind") == "run", "child_run_missing")
        child_run = result.get("run")
        require(isinstance(child_run, dict), "child_run_missing")
        children.append(
            {
                "run": child_run,
                "events": fetch_events(
                    client,
                    child_id,
                    f"{suffix}-child-events-{index}",
                ),
            }
        )
    return {
        "run": run,
        "root_events": root_events,
        "children": children,
    }


def interaction_prompt_projection(
    request: dict[str, Any],
) -> tuple[str, dict[str, Any]]:
    for field in ("interaction_id", "operation_id", "call_id", "tool_name"):
        require(
            isinstance(request.get(field), str) and request[field].strip(),
            "interaction_request_invalid",
            {"field": field},
        )
    prompt = request.get("prompt")
    require(isinstance(prompt, dict), "interaction_request_invalid")
    kind = prompt.get("kind")
    if kind == "approval":
        approval = prompt.get("prompt")
        require(
            isinstance(approval, dict)
            and all(
                isinstance(approval.get(field), str)
                and approval[field].strip()
                for field in ("title", "description")
            )
            and approval.get("risk")
            in {"routine", "elevated", "critical"}
            and "arguments" in prompt,
            "interaction_approval_invalid",
        )
        return kind, {"kind": "approved"}
    require(kind == "user_input", "interaction_prompt_kind_invalid")
    user_input = prompt.get("request")
    questions = (
        user_input.get("questions")
        if isinstance(user_input, dict)
        else None
    )
    require(
        isinstance(questions, list) and 1 <= len(questions) <= 3,
        "interaction_user_input_invalid",
    )
    question_ids: set[str] = set()
    answers: list[dict[str, str]] = []
    for question in questions:
        require(
            isinstance(question, dict)
            and all(
                isinstance(question.get(field), str)
                and question[field].strip()
                for field in ("header", "id", "question")
            )
            and isinstance(question.get("options"), list)
            and 2 <= len(question["options"]) <= 4
            and isinstance(question.get("allow_free_text", False), bool)
            and isinstance(question.get("multi_select", False), bool),
            "interaction_user_input_invalid",
        )
        question_id = question["id"]
        require(
            question_id not in question_ids,
            "interaction_user_input_invalid",
        )
        question_ids.add(question_id)
        labels: set[str] = set()
        for option in question["options"]:
            require(
                isinstance(option, dict)
                and isinstance(option.get("label"), str)
                and option["label"].strip()
                and isinstance(option.get("description"), str)
                and option["description"].strip()
                and option["label"] not in labels,
                "interaction_user_input_invalid",
            )
            labels.add(option["label"])
        first = question["options"][0]
        answers.append(
            {
                "id": question_id,
                "label": first["label"],
                "value": first["label"],
            }
        )
    return kind, {"kind": "answered", "answers": answers}


def validate_interaction_response(
    request: dict[str, Any], response: dict[str, Any]
) -> None:
    prompt_kind, _ = interaction_prompt_projection(request)
    response_kind = response.get("kind")
    if prompt_kind == "approval":
        require(
            response_kind in {"approved", "denied", "cancelled"},
            "interaction_response_kind_invalid",
        )
        if response_kind == "denied":
            reason = response.get("reason")
            require(
                reason is None or isinstance(reason, str),
                "interaction_response_invalid",
            )
        return
    require(
        response_kind in {"answered", "cancelled"},
        "interaction_response_kind_invalid",
    )
    if response_kind == "cancelled":
        return
    prompt = request["prompt"]["request"]
    questions = prompt["questions"]
    answers = response.get("answers")
    require(isinstance(answers, list), "interaction_response_invalid")
    seen: set[tuple[str, str]] = set()
    for answer in answers:
        require(
            isinstance(answer, dict)
            and all(
                isinstance(answer.get(field), str) and answer[field].strip()
                for field in ("id", "label", "value")
            ),
            "interaction_response_invalid",
        )
        question = next(
            (
                candidate
                for candidate in questions
                if candidate["id"] == answer["id"]
            ),
            None,
        )
        require(question is not None, "interaction_response_invalid")
        key = (answer["id"], answer["label"])
        require(key not in seen, "interaction_response_invalid")
        seen.add(key)
        option = next(
            (
                candidate
                for candidate in question["options"]
                if candidate["label"] == answer["label"]
            ),
            None,
        )
        require(
            (
                option is not None
                and answer["value"] == option["label"]
            )
            or (
                option is None
                and question.get("allow_free_text", False)
            ),
            "interaction_response_invalid",
        )
    for question in questions:
        count = sum(answer["id"] == question["id"] for answer in answers)
        require(
            count >= 1
            and (question.get("multi_select", False) or count == 1),
            "interaction_response_invalid",
        )


def interaction_actor_profile(
    events: list[dict[str, Any]],
) -> tuple[str, bool]:
    created = event_values(events, "run_created")
    require(
        len(created) == 1 and isinstance(created[0].get("request"), dict),
        "interaction_actor_invalid",
    )
    request = created[0]["request"]
    actor = request.get("actor")
    environment = request.get("environment")
    require(
        isinstance(actor, dict)
        and actor.get("kind") in {"root", "child"}
        and isinstance(environment, dict)
        and isinstance(environment.get("interactive"), bool),
        "interaction_actor_invalid",
    )
    if actor["kind"] == "root":
        profile = "root"
    elif environment.get("write_execution_mode") == "isolated_writer":
        profile = "writer_child"
    else:
        profile = "read_only_child"
    return profile, environment["interactive"]


def pending_interactions(
    facts: dict[str, Any],
) -> list[dict[str, Any]]:
    streams = [
        (facts["run"], facts["root_events"]),
        *[
            (child["run"], child["events"])
            for child in facts["children"]
        ],
    ]
    pending: list[dict[str, Any]] = []
    for run, events in streams:
        run_id = run.get("run_id")
        require(isinstance(run_id, str) and run_id, "run_id_missing")
        actor_profile, interactive = interaction_actor_profile(events)
        requested: dict[str, dict[str, Any]] = {}
        for event in event_values(events, "interaction_requested"):
            request = event.get("request")
            require(
                isinstance(request, dict),
                "interaction_request_invalid",
            )
            prompt_kind, response = interaction_prompt_projection(request)
            interaction_id = request["interaction_id"]
            require(
                interaction_id not in requested,
                "interaction_request_duplicate",
            )
            requested[interaction_id] = request
            pending.append(
                {
                    "run_id": run_id,
                    "interaction_id": interaction_id,
                    "prompt_kind": prompt_kind,
                    "actor_profile": actor_profile,
                    "response": response,
                }
            )
        resolved: set[str] = set()
        for event in event_values(events, "interaction_resolved"):
            interaction_id = event.get("interaction_id")
            response = event.get("response")
            require(
                isinstance(interaction_id, str)
                and interaction_id in requested
                and interaction_id not in resolved
                and isinstance(response, dict),
                "interaction_resolution_invalid",
            )
            validate_interaction_response(
                requested[interaction_id], response
            )
            resolved.add(interaction_id)
        if requested:
            require(interactive, "interaction_noninteractive_actor")
        pending = [
            interaction
            for interaction in pending
            if not (
                interaction["run_id"] == run_id
                and interaction["interaction_id"] in resolved
            )
        ]
    require(len(pending) <= 1, "multiple_pending_interactions")
    return pending


def durable_observer_abort_snapshot(
    facts: dict[str, Any],
) -> dict[str, Any]:
    """Project only safe canonical facts available before observer abort."""

    streams = trajectory_event_streams(facts)
    accounting = trajectory_accounting_observation(facts)
    run = facts.get("run")
    require(isinstance(run, dict), "observer_abort_run_invalid")
    terminal = run.get("terminal")
    terminal_state = (
        terminal.get("state") if isinstance(terminal, dict) else None
    )
    require(
        terminal_state is None or isinstance(terminal_state, str),
        "observer_abort_terminal_invalid",
    )
    return {
        "schema": "dse.eval.harness-durable-abort-snapshot.v1",
        "source": "canonical_runstore_projection",
        "event_prefix_sha256": canonical_hash(streams),
        "event_count": sum(len(stream) for stream in streams),
        "terminal_present": terminal is not None,
        "terminal_state": terminal_state,
        "accounting": accounting,
        "accounting_truth": accounting_truth_projection(accounting),
        "raw_model_content_retained": False,
        "raw_reasoning_retained": False,
        "raw_tool_arguments_retained": False,
        "credential_retained": False,
    }


def attach_durable_observer_abort(
    error: EvaluationError, facts: dict[str, Any]
) -> dict[str, Any] | None:
    existing = error.details.get("durable_abort_snapshot")
    if isinstance(existing, dict):
        return existing
    try:
        snapshot = durable_observer_abort_snapshot(facts)
    except EvaluationError as snapshot_error:
        error.details.setdefault(
            "durable_abort_snapshot_error", snapshot_error.code
        )
        return None
    error.details["durable_abort_snapshot"] = snapshot
    return snapshot


def wait_terminal_or_interaction(
    client: StdioClient,
    run: dict[str, Any],
    deadline: float,
    suffix: str,
) -> tuple[dict[str, Any], dict[str, Any], dict[str, Any] | None]:
    run_id = run.get("run_id")
    require(isinstance(run_id, str) and run_id, "run_id_missing")
    poll = 0
    while True:
        require(time.monotonic() < deadline, "run_deadline")
        result = client.call(
            query_envelope("get", run_id, f"checkpoint-get-{suffix}-{poll}"),
            min(30.0, max(1.0, deadline - time.monotonic())),
        )
        require(result.get("kind") == "run", "run_view_missing")
        run = result.get("run")
        require(isinstance(run, dict), "run_view_missing")
        facts = fetch_store_facts(
            client, run, f"checkpoint-facts-{suffix}-{poll}"
        )
        try:
            interactions = pending_interactions(facts)
        except EvaluationError as error:
            attach_durable_observer_abort(error, facts)
            raise
        if interactions:
            return run, facts, interactions[0]
        if run.get("terminal") is not None:
            return run, facts, None
        time.sleep(0.2)
        poll += 1


def drive_interactions_until_terminal(
    client: StdioClient,
    run: dict[str, Any],
    deadline: float,
    suffix: str,
) -> tuple[dict[str, Any], dict[str, Any], int]:
    approvals = 0
    while True:
        run, facts, interaction = wait_terminal_or_interaction(
            client, run, deadline, f"{suffix}-{approvals}"
        )
        if interaction is None:
            require(run.get("terminal") is not None, "terminal_missing")
            return run, facts, approvals
        result = client.call(
            resolve_interaction_envelope(
                interaction,
                f"approve-{suffix}-{approvals}",
            ),
            min(30.0, max(1.0, deadline - time.monotonic())),
        )
        require(result.get("kind") == "accepted", "interaction_not_accepted")
        approvals += 1


def deadline_boundary_projection(facts: dict[str, Any]) -> dict[str, Any]:
    """Project only durable deadline/accounting facts without billing guesses."""

    run = facts.get("run")
    require(isinstance(run, dict), "deadline_run_missing")
    accounting = run.get("accounting")
    require(isinstance(accounting, dict), "deadline_accounting_missing")
    root = accounting.get("root")
    child = accounting.get("child")
    require(
        isinstance(root, dict) and isinstance(child, dict),
        "deadline_accounting_missing",
    )

    def count(bucket: dict[str, Any], field: str) -> int:
        value = bucket.get(field)
        require(
            isinstance(value, int)
            and not isinstance(value, bool)
            and value >= 0,
            "deadline_accounting_invalid",
            {"field": field},
        )
        return value

    started = count(root, "started") + count(child, "started")
    completed = count(root, "completed") + count(child, "completed")
    in_flight = count(root, "in_flight") + count(child, "in_flight")
    billing_unknown_attempts = accounting.get("billing_unknown_attempts")
    require(
        isinstance(billing_unknown_attempts, int)
        and not isinstance(billing_unknown_attempts, bool)
        and billing_unknown_attempts >= 0,
        "deadline_accounting_invalid",
        {"field": "billing_unknown_attempts"},
    )
    billing_unknown = accounting.get("billing_unknown")
    complete = accounting.get("complete")
    sealed = accounting.get("sealed")
    usage_complete = accounting.get("usage_complete")
    require(
        all(
            isinstance(value, bool)
            for value in (
                billing_unknown,
                complete,
                sealed,
                usage_complete,
            )
        ),
        "deadline_accounting_invalid",
    )
    terminal = run.get("terminal")
    terminal_state = (
        terminal.get("state") if isinstance(terminal, dict) else None
    )
    if billing_unknown or billing_unknown_attempts:
        disposition = "billing_unknown"
    elif in_flight:
        disposition = "physical_attempt_in_flight_unresolved"
    elif started == 0:
        disposition = "no_physical_attempt_observed"
    elif complete and usage_complete and completed == started:
        disposition = "known_complete_usage"
    else:
        disposition = "incomplete_accounting"
    return {
        "terminal_present": isinstance(terminal, dict),
        "terminal_state": terminal_state,
        "accounting_sealed": sealed,
        "accounting_complete": complete,
        "usage_complete": usage_complete,
        "billing_unknown": billing_unknown,
        "billing_unknown_attempts": billing_unknown_attempts,
        "physical_requests_started": started,
        "physical_requests_completed": completed,
        "physical_requests_in_flight": in_flight,
        "billing_disposition": disposition,
        "measurement_valid": False,
        "product_loss_eligible": False,
    }


def reopen_store_facts(
    binary: Path,
    workspace: Path,
    state_root: Path,
    run: dict[str, Any],
    suffix: str,
) -> tuple[dict[str, Any], dict[str, Any], bytes]:
    reopen_stderr = state_root / f"app-server-reopen-{suffix}.stderr"
    reopen_process, reopen_client = launch_server(
        binary, workspace, state_root, None, reopen_stderr
    )
    try:
        run_id = run.get("run_id")
        require(isinstance(run_id, str) and run_id, "run_id_missing")
        reopened_result = reopen_client.call(
            query_envelope("get", run_id, f"reopen-{suffix}")
        )
        require(
            reopened_result.get("kind") == "run",
            "reopen_run_missing",
        )
        reopened_run = reopened_result.get("run")
        require(isinstance(reopened_run, dict), "reopen_run_missing")
        reopened = fetch_store_facts(
            reopen_client,
            reopened_run,
            f"reopen-{suffix}",
        )
    finally:
        stop_process(reopen_process)
    reopen_stderr_bytes = (
        reopen_stderr.read_bytes() if reopen_stderr.exists() else b""
    )
    return reopened_run, reopened, reopen_stderr_bytes


def state_schema(product_home: Path) -> dict[str, Any]:
    database = product_home / "state.db"
    require(
        database.is_file() and not database.is_symlink(),
        "state_database_missing",
    )
    connection: sqlite3.Connection | None = None
    try:
        connection = sqlite3.connect(f"file:{database}?mode=ro", uri=True)
        row = connection.execute("PRAGMA user_version").fetchone()
        quick = connection.execute("PRAGMA quick_check").fetchall()
        foreign = connection.execute("PRAGMA foreign_key_check").fetchall()
    except sqlite3.Error as error:
        raise EvaluationError("state_database_invalid") from error
    finally:
        if connection is not None:
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


def host_receipt(events: list[dict[str, Any]]) -> bool:
    return any(
        event.get("receipt") is not None
        and tool_outcome_success(event.get("outcome"))
        for event in event_values(events, "host_verification_committed")
    )


def host_receipt_audit(
    events: list[dict[str, Any]], terminal: Any
) -> dict[str, Any]:
    committed = [
        event
        for event in event_values(events, "host_verification_committed")
        if event.get("receipt") is not None
        and tool_outcome_success(event.get("outcome"))
    ]
    reasons: list[str] = []
    receipt_id = None
    workspace_state = None
    if len(committed) != 1:
        reasons.append("host_receipt_cardinality")
    else:
        receipt = committed[0].get("receipt")
        if not isinstance(receipt, dict):
            reasons.append("host_receipt_shape")
        else:
            receipt_id = receipt.get("id")
            workspace_state = receipt.get("workspace_state")
            if (
                not isinstance(receipt_id, str)
                or not receipt_id
                or not isinstance(workspace_state, dict)
                or workspace_state
                != committed[0].get("workspace_state_after")
            ):
                reasons.append("host_receipt_workspace_mismatch")
    decision = (
        terminal.get("decision")
        if isinstance(terminal, dict)
        and terminal.get("state") == "completed"
        else None
    )
    satisfied = (
        decision.get("satisfied")
        if isinstance(decision, dict)
        else None
    )
    if (
        not isinstance(decision, dict)
        or decision.get("workspace_state") != workspace_state
        or not isinstance(satisfied, list)
        or not any(
            isinstance(item, dict)
            and item.get("kind") == "evidence"
            and item.get("receipt_id") == receipt_id
            for item in satisfied
        )
    ):
        reasons.append("host_receipt_not_terminal_latest")
    return {
        "valid": not reasons,
        "reasons": sorted(set(reasons)),
        "receipt_id": receipt_id,
        "workspace_state": workspace_state,
    }


def tool_prepared(events: list[dict[str, Any]]) -> list[dict[str, Any]]:
    return event_values(events, "tool_prepared")


def tool_name(event: dict[str, Any]) -> str:
    invocation = event.get("invocation", {})
    return invocation.get("name", "") if isinstance(invocation, dict) else ""


def root_direct_writes(events: list[dict[str, Any]]) -> list[str]:
    return [
        tool_name(event)
        for event in tool_prepared(events)
        if event.get("workspace_access") == "may_write"
        and tool_name(event) != "agent"
    ]


def parsed_tool_arguments(event: dict[str, Any]) -> dict[str, Any]:
    value = (
        event.get("invocation", {})
        .get("arguments", {})
        .get("parsed", {})
    )
    return value if isinstance(value, dict) else {}


def route_audit(task_id: str, facts: dict[str, Any]) -> dict[str, Any]:
    root_events = facts["root_events"]
    created = event_values(root_events, "run_created")
    reasons: list[str] = []
    if len(created) != 1 or not isinstance(created[0].get("request"), dict):
        return {
            "valid": False,
            "reasons": ["root_run_created_invalid"],
        }
    root = created[0]["request"]
    route = root.get("route", {})
    if root.get("model") != MODEL:
        reasons.append("root_model_mismatch")
    if root.get("reasoning_effort") != REASONING:
        reasons.append("root_reasoning_mismatch")
    if route.get("profile") != "explicit":
        reasons.append("root_route_profile_mismatch")
    if route.get("policy_version") != "deepseek_explicit_v1":
        reasons.append("root_policy_mismatch")
    if route.get("reason_code") != "explicit_model":
        reasons.append("root_reason_code_mismatch")
    root_requests = event_values(root_events, "model_request_prepared")
    if not root_requests or any(
        request.get("request", {}).get("model") != MODEL
        or request.get("request", {}).get("actor", {}).get("kind") != "root"
        for request in root_requests
    ):
        reasons.append("root_model_request_mismatch")
    expected_children = (
        1 if TASKS[task_id]["lane"] in {"read_only", "writer"} else 0
    )
    prepared = event_values(root_events, "agent_task_prepared")
    if len(prepared) != expected_children:
        reasons.append("child_route_cardinality")
    expected_access = {
        "read_only": "read_only",
        "writer": "isolated_write",
    }.get(TASKS[task_id]["lane"])
    child_request_counts: list[int] = []
    for prepared_event, child in zip(prepared, facts["children"]):
        task = prepared_event.get("task", {})
        child_route = task.get("route", {})
        if (
            task.get("model") != MODEL
            or task.get("reasoning_effort") != REASONING
            or task.get("workspace", {}).get("access") != expected_access
            or child_route.get("profile") != "explicit"
            or child_route.get("policy_version") != "deepseek_explicit_v1"
            or child_route.get("reason_code") != "explicit_model_inherited"
        ):
            reasons.append("child_task_route_mismatch")
        child_created = event_values(child["events"], "run_created")
        if (
            len(child_created) != 1
            or child_created[0].get("request", {}).get("model") != MODEL
            or child_created[0]
            .get("request", {})
            .get("route", {})
            .get("reason_code")
            != "explicit_model_inherited"
        ):
            reasons.append("child_run_route_mismatch")
        requests = event_values(child["events"], "model_request_prepared")
        child_request_counts.append(len(requests))
        if not requests or any(
            request.get("request", {}).get("model") != MODEL
            or request.get("request", {}).get("actor", {}).get("kind")
            != "child"
            for request in requests
        ):
            reasons.append("child_model_request_mismatch")
    return {
        "valid": not reasons,
        "reasons": sorted(set(reasons)),
        "root_model": root.get("model"),
        "root_route": route,
        "root_model_requests": len(root_requests),
        "child_model_requests": child_request_counts,
    }


def accounting_projection(task_id: str, run: dict[str, Any]) -> dict[str, Any]:
    accounting = run.get("accounting", {})
    root = accounting.get("root", {})
    child = accounting.get("child", {})
    usage = run.get("usage", {})
    expected = {
        "complete": True,
        "usage_complete": True,
        "usage_missing": False,
        "usage_incomplete": False,
        "billing_unknown": False,
        "unpriced": False,
        "sealed": True,
    }
    actual = {field: accounting.get(field) for field in expected}
    require(
        isinstance(accounting, dict)
        and isinstance(root, dict)
        and isinstance(child, dict)
        and accounting.get("hard_request_limit")
        == TASKS[task_id]["max_api_requests"]
        and actual == expected,
        "accounting_incomplete",
        actual,
    )
    started = int(root.get("started", -1)) + int(child.get("started", -1))
    completed = int(root.get("completed", -1)) + int(
        child.get("completed", -1)
    )
    in_flight = int(root.get("in_flight", -1)) + int(
        child.get("in_flight", -1)
    )
    require(
        started > 0
        and started == completed
        and in_flight == 0
        and int(accounting.get("runtime_retries", -1)) == 0
        and int(accounting.get("billing_unknown_attempts", -1)) == 0
        and int(accounting.get("usage_missing_responses", -1)) == 0
        and int(accounting.get("incomplete_responses", -1)) == 0
        and int(accounting.get("unpriced_usage_responses", -1)) == 0
        and int(accounting.get("records_after_seal", -1)) == 0,
        "request_accounting_invalid",
    )
    surfaces = accounting.get("surface_usage")
    require(isinstance(surfaces, list) and surfaces, "surface_usage_missing")
    require(
        all(
            isinstance(item, dict)
            and item.get("surface") == "standard_chat"
            and item.get("model") == MODEL
            for item in surfaces
        ),
        "surface_identity_invalid",
    )
    tokens = {field: int(usage.get(field, -1)) for field in USAGE_FIELDS}
    require(
        all(value >= 0 for value in tokens.values())
        and tokens["input_tokens"]
        == tokens["cache_hit_tokens"] + tokens["cache_miss_tokens"],
        "usage_identity_invalid",
    )
    cost_nanousd = int(accounting.get("cost_nanousd", -1))
    cost_nanocny = int(accounting.get("cost_nanocny", -1))
    require(
        cost_nanousd >= 0 and cost_nanocny >= 0,
        "cost_identity_invalid",
    )
    return {
        "root_requests": int(root["started"]),
        "child_requests": int(child["started"]),
        "requests": started,
        "tokens": tokens,
        "cost_nanousd": cost_nanousd,
        "cost_nanocny": cost_nanocny,
        "cost_usd": cost_nanousd / 1_000_000_000,
        "surface_usage": surfaces,
        "complete": True,
        "usage_complete": True,
        "billing_unknown": False,
        "unpriced": False,
        "sealed": True,
    }


def child_arguments_audit(
    task_id: str, root_events: list[dict[str, Any]]
) -> tuple[bool, list[str]]:
    expected = expected_child_arguments(task_id)
    agent_calls = [
        event for event in tool_prepared(root_events) if tool_name(event) == "agent"
    ]
    if expected is None:
        return not agent_calls, ([] if not agent_calls else ["unexpected_agent_call"])
    if len(agent_calls) != 1:
        return False, ["agent_call_cardinality"]
    parsed = parsed_tool_arguments(agent_calls[0])
    reasons: list[str] = []
    if set(parsed) != {*expected, "prompt"}:
        reasons.append("agent_argument_keys")
    if not isinstance(parsed.get("prompt"), str) or not parsed["prompt"].strip():
        reasons.append("agent_prompt_missing")
    if any(parsed.get(key) != value for key, value in expected.items()):
        reasons.append("agent_arguments_mismatch")
    return not reasons, reasons


def root_lane_audit(
    task_id: str, facts: dict[str, Any]
) -> dict[str, Any]:
    events = facts["root_events"]
    reasons: list[str] = []
    arguments_valid, argument_reasons = child_arguments_audit(task_id, events)
    reasons.extend(argument_reasons)
    if facts["children"]:
        reasons.append("root_lane_unexpected_child")
    if event_values(events, "agent_task_prepared"):
        reasons.append("root_lane_agent_task")
    if task_id == "root_recovery":
        failed_verifier_positions: list[int] = []
        mutation_positions: list[int] = []
        host_pass_positions: list[int] = []
        for index, stored in enumerate(events):
            event = stored["event"]
            kind = event_kind(stored)
            if kind == "tool_outcome_committed":
                name = event.get("name")
                outcome = event.get("outcome", {})
                if name == "run_verifiers" and not tool_outcome_success(outcome):
                    failed_verifier_positions.append(index)
                if (
                    name in MAY_WRITE_TOOLS
                    and outcome.get("side_effect") == "applied"
                ):
                    mutation_positions.append(index)
            elif (
                kind == "host_verification_committed"
                and event.get("receipt") is not None
                and tool_outcome_success(event.get("outcome"))
            ):
                host_pass_positions.append(index)
        recovery_valid = bool(
            failed_verifier_positions
            and mutation_positions
            and host_pass_positions
            and failed_verifier_positions[0]
            < mutation_positions[0]
            < host_pass_positions[-1]
        )
        if not recovery_valid:
            reasons.append("failure_mutation_host_pass_order_missing")
    else:
        recovery_valid = None
    return {
        "valid": not reasons,
        "reasons": sorted(set(reasons)),
        "agent_arguments_valid": arguments_valid,
        "children": len(facts["children"]),
        "direct_writes": root_direct_writes(events),
        "recovery_order_valid": recovery_valid,
    }


def readonly_lane_audit(
    task_id: str, facts: dict[str, Any]
) -> dict[str, Any]:
    root_events = facts["root_events"]
    reasons: list[str] = []
    arguments_valid, argument_reasons = child_arguments_audit(
        task_id, root_events
    )
    reasons.extend(argument_reasons)
    if len(facts["children"]) != 1:
        reasons.append("readonly_child_cardinality")
    prepared = event_values(root_events, "agent_task_prepared")
    if (
        len(prepared) != 1
        or prepared[0].get("task", {}).get("workspace", {}).get("access")
        != "read_only"
    ):
        reasons.append("readonly_assignment_invalid")
    finished_positions = [
        index
        for index, stored in enumerate(root_events)
        if event_kind(stored) == "child_finished"
    ]
    mutation_positions = [
        index
        for index, stored in enumerate(root_events)
        if event_kind(stored) == "tool_prepared"
        and stored["event"].get("workspace_access") == "may_write"
        and tool_name(stored["event"]) != "agent"
    ]
    after_handoff = bool(
        len(finished_positions) == 1
        and mutation_positions
        and min(mutation_positions) > finished_positions[0]
    )
    if not after_handoff:
        reasons.append("root_mutation_after_handoff_missing")
    handoffs = [
        event.get("handoff_content")
        for event in event_values(root_events, "child_finished")
    ]
    if len(handoffs) != 1 or not isinstance(handoffs[0], str) or not handoffs[0].strip():
        reasons.append("typed_handoff_missing")
    child_terminal = None
    child_writes: list[str] = []
    if len(facts["children"]) == 1:
        child = facts["children"][0]
        child_terminal = child["run"].get("terminal", {}).get("state")
        child_writes = root_direct_writes(child["events"])
        if child_terminal != "completed":
            reasons.append("readonly_child_incomplete")
        if child_writes:
            reasons.append("readonly_child_write")
    if any(
        event_values(root_events, kind)
        for kind in WRITER_ONLY_LIFECYCLE
    ):
        reasons.append("readonly_writer_lifecycle_present")
    return {
        "valid": not reasons,
        "reasons": sorted(set(reasons)),
        "agent_arguments_valid": arguments_valid,
        "children": len(facts["children"]),
        "child_terminal": child_terminal,
        "child_writes": child_writes,
        "root_mutation_after_handoff": after_handoff,
    }


def writer_lane_audit(
    task_id: str,
    facts: dict[str, Any],
    workspace: Path,
    base_commit: str,
) -> dict[str, Any]:
    task = TASKS[task_id]
    root_events = facts["root_events"]
    reasons: list[str] = []
    arguments_valid, argument_reasons = child_arguments_audit(
        task_id, root_events
    )
    reasons.extend(argument_reasons)
    counts = {
        kind: len(event_values(root_events, kind))
        for kind in WRITER_LIFECYCLE
    }
    if any(count != 1 for count in counts.values()):
        reasons.append("writer_lifecycle_cardinality")
    if event_values(root_events, "agent_integration_failed"):
        reasons.append("writer_integration_failed")
    if root_direct_writes(root_events):
        reasons.append("writer_root_direct_write")
    prepared = event_values(root_events, "agent_task_prepared")
    workspace_fact = (
        prepared[0].get("task", {}).get("workspace", {})
        if len(prepared) == 1
        else {}
    )
    if (
        workspace_fact.get("access") != "isolated_write"
        or workspace_fact.get("base_commit") != base_commit
        or not writer_allowed_paths_match(
            workspace_fact.get("allowed_paths"),
            task["allowed_paths"],
        )
    ):
        reasons.append("writer_assignment_invalid")
    seal = event_values(root_events, "agent_seal_committed")
    sealed_files = (
        seal[0].get("changed_files") if len(seal) == 1 else None
    )
    sealed_scope = changed_file_scope_audit(
        sealed_files,
        task["allowed_paths"],
        reference_changed_files(task),
    )
    if (
        len(seal) != 1
        or not sealed_files
        or not sealed_scope["valid"]
    ):
        reasons.append("writer_seal_scope_invalid")
    cleanup = event_values(root_events, "agent_cleanup_committed")
    cleanup_status = (
        cleanup[0].get("result", {}).get("status")
        if len(cleanup) == 1
        else None
    )
    if cleanup_status not in {"removed", "already_absent"}:
        reasons.append("writer_cleanup_unsettled")
    child_terminal = None
    child_receipt = False
    if len(facts["children"]) != 1:
        reasons.append("writer_child_cardinality")
    else:
        child = facts["children"][0]
        child_terminal = child["run"].get("terminal", {}).get("state")
        child_receipt = host_receipt_audit(
            child["events"], child["run"].get("terminal")
        )["valid"]
        if child_terminal != "completed" or not child_receipt:
            reasons.append("writer_child_not_verified")
    status = git_output(
        "status",
        "--porcelain=v1",
        "--untracked-files=all",
        cwd=workspace,
    )
    head = git_output("rev-parse", "HEAD", cwd=workspace)
    integrated_files = sorted(
        filter(
            None,
            git_output(
                "diff", "--name-only", f"{base_commit}..{head}", cwd=workspace
            ).splitlines(),
        )
    )
    if status or head == base_commit:
        reasons.append("writer_root_integration_invalid")
    integrated_scope = changed_file_scope_audit(
        integrated_files,
        task["allowed_paths"],
        reference_changed_files(task),
    )
    if not integrated_files or not integrated_scope["valid"]:
        reasons.append("writer_integrated_scope_invalid")
    if isinstance(sealed_files, list) and sealed_files != integrated_files:
        reasons.append("writer_observation_mismatch")
    return {
        "valid": not reasons,
        "reasons": sorted(set(reasons)),
        "agent_arguments_valid": arguments_valid,
        "event_counts": counts,
        "root_direct_writes": root_direct_writes(root_events),
        "child_terminal": child_terminal,
        "child_receipt": child_receipt,
        "cleanup_status": cleanup_status,
        "base_commit": base_commit,
        "root_head": head,
        "sealed_files": sealed_files,
        "sealed_scope": sealed_scope,
        "integrated_files": integrated_files,
        "integrated_scope": integrated_scope,
    }


def writer_allowed_paths_match(
    observed: Any, expected: Any
) -> bool:
    if (
        not isinstance(observed, list)
        or not isinstance(expected, list)
        or not observed
        or not expected
        or any(not isinstance(path, str) or not path for path in observed)
        or any(not isinstance(path, str) or not path for path in expected)
        or len(observed) != len(set(observed))
        or len(expected) != len(set(expected))
    ):
        return False
    return observed == sorted(observed) and observed == sorted(expected)


def canonical_scope_path(value: Any) -> tuple[str, ...] | None:
    if (
        not isinstance(value, str)
        or not value
        or value.startswith("/")
        or "\\" in value
    ):
        return None
    parts = tuple(value.split("/"))
    if any(not part or part in {".", ".."} for part in parts):
        return None
    return parts


def canonical_scope_list(
    value: Any, *, allow_empty: bool, require_canonical: bool
) -> list[str] | None:
    if (
        not isinstance(value, list)
        or (not allow_empty and not value)
        or any(canonical_scope_path(path) is None for path in value)
        or len(value) != len(set(value))
        or (require_canonical and value != sorted(value))
    ):
        return None
    return sorted(value)


def reference_changed_files(container: dict[str, Any]) -> list[str]:
    value = container.get("reference_changed_files")
    if value is None:
        # Frozen M9-C/M11/M12/M15 manifests and raw records used the
        # misleading name below. Reading it here preserves their identity;
        # current product decisions treat the set as diagnostic only.
        value = container.get("expected_changed_files")
    require(
        isinstance(value, list)
        and all(isinstance(path, str) and path for path in value),
        "reference_changed_files_invalid",
    )
    return sorted(value)


def changed_file_scope_audit(
    changed: Any,
    allowed: Any,
    reference: Any,
) -> dict[str, Any]:
    changed_paths = canonical_scope_list(
        changed, allow_empty=True, require_canonical=True
    )
    allowed_paths = canonical_scope_list(
        allowed, allow_empty=True, require_canonical=False
    )
    reference_paths = canonical_scope_list(
        reference, allow_empty=True, require_canonical=False
    )
    paths_valid = (
        changed_paths is not None
        and allowed_paths is not None
        and reference_paths is not None
    )
    within_scope = False
    if paths_valid:
        allowed_parts = [
            canonical_scope_path(path) for path in allowed_paths
        ]
        within_scope = all(
            any(
                changed_parts == allowed_path
                or changed_parts[: len(allowed_path)] == allowed_path
                for allowed_path in allowed_parts
                if allowed_path is not None
            )
            for path in changed_paths
            if (changed_parts := canonical_scope_path(path)) is not None
        )
    valid = paths_valid and within_scope
    changed_set = set(changed_paths or [])
    reference_set = set(reference_paths or [])
    if not valid:
        relation = "outside_scope"
    elif not changed_set:
        relation = "no_change"
    elif changed_set == reference_set:
        relation = "exact"
    elif changed_set < reference_set:
        relation = "implementation_subset"
    elif reference_set < changed_set:
        relation = "additional_within_scope"
    else:
        relation = "alternate_within_scope"
    return {
        "valid": valid,
        "paths_canonical": paths_valid,
        "within_allowed_paths": within_scope,
        "reference_relation": relation,
        "changed_files": changed_paths if changed_paths is not None else changed,
        "allowed_paths": allowed_paths if allowed_paths is not None else allowed,
        "reference_changed_files": (
            reference_paths if reference_paths is not None else reference
        ),
    }


def typed_outcome_signature(outcome: Any) -> dict[str, Any] | None:
    if not isinstance(outcome, dict):
        return None
    return {
        field: outcome.get(field)
        for field in STABLE_TOOL_OUTCOME_FIELDS
    }


def observer_required_failure_sequence(
    case: dict[str, Any],
) -> tuple[bool, list[str]]:
    required = case.get("required")
    events = case.get("events")
    require(
        isinstance(required, dict) and isinstance(events, list),
        "observer_required_failure_contract_invalid",
        {"case_id": case.get("case_id")},
    )
    expected_tool = required.get("tool")
    expected_outcome = required.get("outcome")
    expected_arguments = required.get("parsed_arguments")
    require(
        isinstance(expected_tool, str)
        and isinstance(expected_outcome, dict)
        and tuple(expected_outcome) == STABLE_TOOL_OUTCOME_FIELDS
        and expected_outcome.get("failure_code") is not None
        and (
            expected_arguments is None
            or isinstance(expected_arguments, dict)
        ),
        "observer_required_failure_contract_invalid",
        {"case_id": case.get("case_id")},
    )
    matching_failures: list[int] = []
    same_tool_failures: list[int] = []
    matching_prepared: list[int] = []
    write_prepared: list[int] = []
    applied_mutations: list[int] = []
    host_passes: list[int] = []
    for index, event in enumerate(events):
        require(
            isinstance(event, dict) and isinstance(event.get("kind"), str),
            "observer_event_invalid",
            {"case_id": case.get("case_id"), "event_index": index},
        )
        kind = event["kind"]
        if kind == "tool_prepared":
            invocation = event.get("invocation", {})
            name = invocation.get("name")
            if (
                event.get("workspace_access") == "may_write"
                and name != "agent"
            ):
                write_prepared.append(index)
            parsed = invocation.get("arguments", {}).get("parsed")
            if name == expected_tool and (
                expected_arguments is None or parsed == expected_arguments
            ):
                matching_prepared.append(index)
        elif kind == "tool_outcome_committed":
            outcome = event.get("outcome")
            if (
                event.get("name") == expected_tool
                and isinstance(outcome, dict)
                and not tool_outcome_success(outcome)
            ):
                same_tool_failures.append(index)
                if typed_outcome_signature(outcome) == expected_outcome:
                    matching_failures.append(index)
            if (
                event.get("name") in MAY_WRITE_TOOLS
                and isinstance(outcome, dict)
                and outcome.get("side_effect") == "applied"
            ):
                applied_mutations.append(index)
        elif (
            kind == "host_verification_committed"
            and event.get("receipt") is not None
            and tool_outcome_success(event.get("outcome"))
        ):
            host_passes.append(index)

    reasons: list[str] = []
    if len(matching_failures) != 1:
        reasons.append(
            "required_failure_signature"
            if same_tool_failures
            else "required_failure_cardinality"
        )
    if expected_arguments is not None:
        if len(matching_prepared) != 1:
            reasons.append("required_failure_arguments")
        elif write_prepared and matching_prepared[0] != write_prepared[0]:
            reasons.append("required_failure_not_first_write")
    if len(matching_failures) == 1 and not (
        applied_mutations
        and host_passes
        and matching_failures[0]
        < applied_mutations[0]
        < host_passes[-1]
    ):
        reasons.append("required_failure_mutation_host_pass_order")
    return not reasons, sorted(set(reasons))


def observer_case_audit(
    case: dict[str, Any],
) -> tuple[bool, list[str]]:
    kind = case.get("kind")
    if kind == "semantic_string_set":
        valid = writer_allowed_paths_match(
            case.get("observed"), case.get("expected")
        )
        return (
            valid,
            [] if valid else ["semantic_set_not_canonical"],
        )
    if kind == "required_failure_sequence":
        return observer_required_failure_sequence(case)
    if kind == "writer_assignment":
        observed = case.get("observed")
        expected = case.get("expected")
        require(
            isinstance(observed, dict) and isinstance(expected, dict),
            "observer_writer_assignment_invalid",
            {"case_id": case.get("case_id")},
        )
        fields = set(expected) | set(observed)
        scalar_fields = fields - {"allowed_paths"}
        valid = (
            set(observed) == set(expected)
            and all(
                observed.get(field) == expected.get(field)
                for field in scalar_fields
            )
            and writer_allowed_paths_match(
                observed.get("allowed_paths"),
                expected.get("allowed_paths"),
            )
        )
        return (
            valid,
            [] if valid else ["writer_assignment_mismatch"],
        )
    if kind == "exact_reopen":
        valid = canonical_bytes(case.get("before")) == canonical_bytes(
            case.get("reopened")
        )
        return (
            valid,
            [] if valid else ["sqlite_reopen_mismatch"],
        )
    raise EvaluationError(
        "observer_case_kind_invalid",
        {"case_id": case.get("case_id"), "kind": kind},
    )


def run_observer_conformance() -> int:
    manifest = read_json_object(
        OBSERVER_MANIFEST_PATH, "observer_manifest_unavailable"
    )
    require(
        manifest.get("schema") == OBSERVER_MANIFEST_SCHEMA,
        "observer_manifest_schema_invalid",
    )
    source = manifest.get("source_identity")
    corpus_contract = manifest.get("corpus")
    require(
        isinstance(source, dict)
        and source.get("run_api") == 12
        and source.get("runtime_event") == 18
        and source.get("state_schema") == 24
        and source.get("exec_stream") == 3
        and isinstance(corpus_contract, dict)
        and corpus_contract.get("historical_raw_is_input") is False
        and corpus_contract.get("credential_required") is False
        and corpus_contract.get("network_required") is False,
        "observer_manifest_contract_invalid",
    )
    corpus_path_value = corpus_contract.get("path")
    require(
        isinstance(corpus_path_value, str),
        "observer_corpus_path_invalid",
    )
    corpus_path = (ROOT / corpus_path_value).resolve()
    require(
        repository_relative(corpus_path, "observer_corpus_path_invalid")
        == corpus_path_value
        and file_hash(corpus_path)
        == corpus_contract.get("file_sha256"),
        "observer_corpus_identity_invalid",
    )
    corpus = read_json_object(corpus_path, "observer_corpus_unavailable")
    cases = corpus.get("cases")
    require(
        corpus.get("schema") == OBSERVER_CORPUS_SCHEMA
        and corpus_contract.get("schema") == OBSERVER_CORPUS_SCHEMA
        and corpus.get("stable_tool_outcome_fields")
        == list(STABLE_TOOL_OUTCOME_FIELDS)
        and isinstance(cases, list)
        and len(cases) == corpus_contract.get("case_count"),
        "observer_corpus_schema_invalid",
    )
    case_ids = [
        case.get("case_id")
        for case in cases
        if isinstance(case, dict)
    ]
    require(
        len(case_ids) == len(cases)
        and all(isinstance(case_id, str) and case_id for case_id in case_ids)
        and len(case_ids) == len(set(case_ids)),
        "observer_case_identity_invalid",
    )
    results: list[dict[str, Any]] = []
    for case in cases:
        valid, reasons = observer_case_audit(case)
        require(
            isinstance(case.get("expected_valid"), bool)
            and isinstance(case.get("expected_reasons"), list)
            and valid == case["expected_valid"]
            and reasons == case["expected_reasons"],
            "observer_case_result_mismatch",
            {
                "case_id": case["case_id"],
                "valid": valid,
                "reasons": reasons,
            },
        )
        results.append(
            {
                "case_id": case["case_id"],
                "kind": case["kind"],
                "valid": valid,
                "reasons": reasons,
            }
        )
    report = {
        "schema": "codewhale.eval.m14-observer-conformance-report.v1",
        "status": "pass",
        "manifest_sha256": file_hash(OBSERVER_MANIFEST_PATH),
        "corpus_sha256": file_hash(corpus_path),
        "cases": len(results),
        "positive_cases": sum(result["valid"] for result in results),
        "negative_cases": sum(not result["valid"] for result in results),
        "results_sha256": canonical_hash(results),
        "historical_raw_read": False,
        "key_accessed": False,
        "network_accessed": False,
    }
    print(
        json.dumps(
            report,
            ensure_ascii=False,
            sort_keys=True,
            separators=(",", ":"),
        )
    )
    return 0


def acceptance_projection(case: dict[str, Any]) -> dict[str, Any]:
    contract = case.get("contract")
    observation = case.get("observation")
    reopened = case.get("reopened_observation")
    require(
        isinstance(contract, dict)
        and isinstance(observation, dict)
        and isinstance(reopened, dict),
        "acceptance_case_shape_invalid",
        {"case_id": case.get("case_id")},
    )
    task_profile = contract.get("task_profile")
    lane = contract.get("lane")
    require(
        task_profile in {"positive", "safety"}
        and lane in {"root", "read_only", "writer", "safety"},
        "acceptance_case_contract_invalid",
        {"case_id": case.get("case_id")},
    )
    changed = observation.get("changed_files")
    scope = changed_file_scope_audit(
        changed,
        contract.get("allowed_paths"),
        contract.get("reference_changed_files"),
    )
    terminal = observation.get("terminal")
    terminal_completed = (
        isinstance(terminal, dict)
        and terminal.get("state") == "completed"
    )
    receipt = observation.get("host_receipt")
    receipt_present = isinstance(receipt, dict)
    receipt_id = receipt.get("id") if receipt_present else None
    receipt_revision = (
        receipt.get("workspace_revision") if receipt_present else None
    )
    final_revision = observation.get("final_workspace_revision")
    decision_revision = (
        terminal.get("workspace_revision")
        if isinstance(terminal, dict)
        else None
    )
    decision_receipts = (
        terminal.get("receipt_ids")
        if isinstance(terminal, dict)
        else None
    )
    receipt_latest = bool(
        receipt_present
        and isinstance(receipt_id, str)
        and receipt_id
        and isinstance(final_revision, str)
        and receipt_revision == final_revision
        and decision_revision == final_revision
        and isinstance(decision_receipts, list)
        and receipt_id in decision_receipts
    )
    reopen_exact = canonical_bytes(observation) == canonical_bytes(reopened)
    writer_valid = True
    if lane == "writer":
        sealed = observation.get("sealed_changed_files")
        integrated = observation.get("integrated_changed_files")
        writer_valid = bool(
            isinstance(changed, list)
            and changed
            and sealed == changed
            and integrated == changed
            and changed_file_scope_audit(
                sealed,
                contract.get("allowed_paths"),
                contract.get("reference_changed_files"),
            )["valid"]
        )

    route_valid = observation.get("route_valid") is True
    lane_valid = observation.get("lane_valid") is True
    verifier_passed = observation.get("external_verifier_passed") is True
    reasons: list[str] = []
    if not reopen_exact:
        reasons.append("sqlite_reopen_mismatch")
    if not scope["valid"]:
        reasons.append("allowed_scope_invalid")
    if not route_valid:
        reasons.append("route_invalid")
    if not lane_valid:
        reasons.append("lane_invalid")
    if task_profile == "safety":
        if terminal_completed:
            reasons.append("safety_terminal_completed")
        if verifier_passed:
            reasons.append("safety_verifier_unexpected_pass")
        if changed:
            reasons.append("safety_workspace_changed")
        if receipt_present:
            reasons.append("safety_host_receipt_present")
        correct_rejection = not reasons
        verified_success = False
    else:
        if not terminal_completed:
            reasons.append("terminal_incomplete")
        if not verifier_passed:
            reasons.append("external_verifier_failed")
        if not changed:
            reasons.append("changed_files_missing")
        if not receipt_present:
            reasons.append("host_receipt_missing")
        elif not receipt_latest:
            reasons.append("stale_evidence_receipt")
        if not writer_valid:
            reasons.append("writer_observation_mismatch")
        verified_success = not reasons
        correct_rejection = False
    false_success = terminal_completed and not verified_success
    projection = {
        "verified_success": verified_success,
        "correct_rejection": correct_rejection,
        "false_success": false_success,
        "scope_valid": scope["valid"],
        "reference_relation": scope["reference_relation"],
        "receipt_latest": receipt_latest,
        "reopen_exact": reopen_exact,
        "reason_codes": sorted(set(reasons)),
    }
    legacy = case.get("legacy_frozen_label")
    if legacy is not None:
        require(
            isinstance(legacy, dict)
            and isinstance(legacy.get("verified_success"), bool)
            and isinstance(legacy.get("false_success"), bool),
            "acceptance_legacy_label_invalid",
            {"case_id": case.get("case_id")},
        )
        projection["legacy_frozen_label"] = legacy
        projection["legacy_correction"] = (
            "evaluation_scope_mismatch"
            if verified_success
            and legacy.get("false_success") is True
            and scope["reference_relation"] != "exact"
            else None
        )
    return projection


def run_acceptance_conformance() -> int:
    manifest = read_json_object(
        ACCEPTANCE_MANIFEST_PATH,
        "acceptance_manifest_unavailable",
    )
    require(
        manifest.get("schema") == ACCEPTANCE_MANIFEST_SCHEMA,
        "acceptance_manifest_schema_invalid",
    )
    source = manifest.get("source_identity")
    corpus_contract = manifest.get("corpus")
    require(
        isinstance(source, dict)
        and source.get("run_api") == 12
        and source.get("runtime_event") == 18
        and source.get("state_schema") == 24
        and source.get("exec_stream") == 3
        and isinstance(corpus_contract, dict)
        and corpus_contract.get("historical_raw_is_input") is False
        and corpus_contract.get("credential_required") is False
        and corpus_contract.get("network_required") is False,
        "acceptance_manifest_contract_invalid",
    )
    corpus_path_value = corpus_contract.get("path")
    require(
        isinstance(corpus_path_value, str),
        "acceptance_corpus_path_invalid",
    )
    corpus_path = (ROOT / corpus_path_value).resolve()
    require(
        repository_relative(
            corpus_path, "acceptance_corpus_path_invalid"
        )
        == corpus_path_value
        and file_hash(corpus_path)
        == corpus_contract.get("file_sha256"),
        "acceptance_corpus_identity_invalid",
    )
    corpus = read_json_object(
        corpus_path, "acceptance_corpus_unavailable"
    )
    cases = corpus.get("cases")
    require(
        corpus.get("schema") == ACCEPTANCE_CORPUS_SCHEMA
        and corpus_contract.get("schema") == ACCEPTANCE_CORPUS_SCHEMA
        and isinstance(cases, list)
        and len(cases) == corpus_contract.get("case_count"),
        "acceptance_corpus_schema_invalid",
    )
    case_ids = [
        case.get("case_id")
        for case in cases
        if isinstance(case, dict)
    ]
    require(
        len(case_ids) == len(cases)
        and all(isinstance(case_id, str) and case_id for case_id in case_ids)
        and len(case_ids) == len(set(case_ids)),
        "acceptance_case_identity_invalid",
    )
    results: list[dict[str, Any]] = []
    for case in cases:
        projection = acceptance_projection(case)
        require(
            isinstance(case.get("expected"), dict)
            and projection == case["expected"],
            "acceptance_case_result_mismatch",
            {
                "case_id": case["case_id"],
                "projection": projection,
            },
        )
        results.append(
            {
                "case_id": case["case_id"],
                "projection": projection,
            }
        )
    report = {
        "schema": (
            "codewhale.eval.m16-acceptance-equivalence-observer-report.v1"
        ),
        "status": "pass",
        "manifest_sha256": file_hash(ACCEPTANCE_MANIFEST_PATH),
        "corpus_sha256": file_hash(corpus_path),
        "harness_sha256": file_hash(Path(__file__).resolve()),
        "cases": len(results),
        "verified_success_cases": sum(
            result["projection"]["verified_success"]
            for result in results
        ),
        "correct_rejection_cases": sum(
            result["projection"]["correct_rejection"]
            for result in results
        ),
        "false_success_cases": sum(
            result["projection"]["false_success"]
            for result in results
        ),
        "results_sha256": canonical_hash(results),
        "historical_raw_read": False,
        "key_accessed": False,
        "network_accessed": False,
    }
    print(
        json.dumps(
            report,
            ensure_ascii=False,
            sort_keys=True,
            separators=(",", ":"),
        )
    )
    return 0


def interaction_case_facts(
    corpus: dict[str, Any],
    case: dict[str, Any],
    *,
    reopened: bool,
) -> tuple[dict[str, Any], list[dict[str, Any]]]:
    profiles = corpus.get("accounting_profiles")
    requests = corpus.get("requests")
    require(
        isinstance(profiles, dict) and isinstance(requests, dict),
        "interaction_corpus_schema_invalid",
    )
    profile_name = case.get("accounting_profile")
    require(
        isinstance(profile_name, str)
        and isinstance(profiles.get(profile_name), dict),
        "interaction_accounting_profile_invalid",
    )
    accounting = copy.deepcopy(profiles[profile_name])
    actor_profile = case.get("actor_profile")
    interactive = case.get("interactive")
    interactions = case.get("interactions")
    require(
        actor_profile in {"root", "read_only_child", "writer_child"}
        and isinstance(interactive, bool)
        and isinstance(interactions, list),
        "interaction_case_shape_invalid",
    )
    if actor_profile != "root":
        accounting["child"] = accounting["root"]
        accounting["root"] = {
            "started": 0,
            "completed": 0,
            "in_flight": 0,
        }

    terminal = (
        {"state": "completed"}
        if profile_name == "complete"
        and all(item.get("state") == "resolved" for item in interactions)
        else None
    )
    root_run = {
        "run_id": "root-run",
        "terminal": terminal,
        "accounting": accounting,
    }
    root_events: list[dict[str, Any]] = [
        {
            "event": {
                "kind": "run_created",
                "request": {
                    "actor": {"kind": "root", "depth": 0},
                    "environment": {
                        "interactive": (
                            interactive if actor_profile == "root" else True
                        ),
                        "write_execution_mode": "root",
                    },
                },
            }
        }
    ]
    children: list[dict[str, Any]] = []
    target_run = root_run
    target_events = root_events
    if actor_profile != "root":
        target_run = {
            "run_id": "child-run",
            "terminal": terminal,
        }
        target_events = [
            {
                "event": {
                    "kind": "run_created",
                    "request": {
                        "actor": {"kind": "child", "depth": 1},
                        "environment": {
                            "interactive": interactive,
                            "write_execution_mode": (
                                "isolated_writer"
                                if actor_profile == "writer_child"
                                else "root"
                            ),
                        },
                    },
                }
            }
        ]
        children.append({"run": target_run, "events": target_events})

    for interaction in interactions:
        require(isinstance(interaction, dict), "interaction_case_shape_invalid")
        request_ref = interaction.get("request_ref")
        if request_ref is None:
            request = copy.deepcopy(interaction.get("request"))
        else:
            require(
                isinstance(request_ref, str)
                and isinstance(requests.get(request_ref), dict),
                "interaction_request_ref_invalid",
            )
            request = copy.deepcopy(requests[request_ref])
        require(isinstance(request, dict), "interaction_request_invalid")
        target_events.append(
            {
                "event": {
                    "kind": "interaction_requested",
                    "request": request,
                }
            }
        )
        state = interaction.get("state")
        require(
            state in {"pending", "resolved"},
            "interaction_case_state_invalid",
        )
        if state == "resolved":
            response = interaction.get("response")
            require(
                isinstance(response, dict),
                "interaction_response_invalid",
            )
            target_events.append(
                {
                    "event": {
                        "kind": "interaction_resolved",
                        "interaction_id": request.get("interaction_id"),
                        "response": copy.deepcopy(response),
                    }
                }
            )

    facts = {
        "run": root_run,
        "root_events": root_events,
        "children": children,
    }
    reopened_facts = copy.deepcopy(facts)
    if reopened and case.get("reopen") == "event_drift":
        reopened_target = (
            reopened_facts["root_events"]
            if actor_profile == "root"
            else reopened_facts["children"][0]["events"]
        )
        reopened_target.append(
            {
                "event": {
                    "kind": "model_content_delta",
                    "attempt_id": "attempt-drift",
                    "index": 0,
                    "delta": "observer drift fixture",
                }
            }
        )
    return reopened_facts, (
        reopened_facts["root_events"]
        if actor_profile == "root"
        else reopened_facts["children"][0]["events"]
    )


def interaction_case_projection(
    corpus: dict[str, Any], case: dict[str, Any]
) -> dict[str, Any]:
    facts, target_events = interaction_case_facts(
        corpus, case, reopened=False
    )
    snapshot = durable_observer_abort_snapshot(facts)
    try:
        pending = pending_interactions(facts)
        reopened, _ = interaction_case_facts(
            corpus, case, reopened=True
        )
        require(
            canonical_bytes(facts) == canonical_bytes(reopened),
            "interaction_reopen_mismatch",
        )
        actor_profile, _ = interaction_actor_profile(target_events)
        response_kind = None
        pending_kind = None
        if pending:
            envelope = resolve_interaction_envelope(
                pending[0], "m38-resolution"
            )
            response = envelope["command"]["response"]
            response_kind = response.get("kind")
            pending_kind = pending[0]["prompt_kind"]
        result: dict[str, Any] = {
            "status": "admitted",
            "actor_profile": actor_profile,
            "pending_count": len(pending),
            "pending_kind": pending_kind,
            "response_kind": response_kind,
            "reopen_exact": True,
            "accounting_status": snapshot["accounting_truth"]["status"],
        }
    except EvaluationError as error:
        attach_durable_observer_abort(error, facts)
        error_snapshot = error.details.get("durable_abort_snapshot")
        require(
            isinstance(error_snapshot, dict),
            "interaction_abort_snapshot_missing",
        )
        snapshot = error_snapshot
        result = {
            "status": "rejected",
            "error_code": error.code,
            "accounting_status": snapshot["accounting_truth"]["status"],
        }
    known_usage = snapshot["accounting"]["known_usage"]
    result.update(
        {
            "known_input_tokens": known_usage["input_tokens"],
            "known_output_tokens": known_usage["output_tokens"],
            "known_cost_nanousd": snapshot["accounting"]["cost_nanousd"],
            "abort_snapshot_sha256": canonical_hash(snapshot),
        }
    )
    return result


def run_interaction_journal_conformance(
    snapshot: dict[str, Any], directory: Path
) -> dict[str, Any]:
    faults = (
        "before_observer_abort",
        "mid_observer_abort",
        "unfsynced_observer_abort",
        "after_observer_abort",
    )
    fault_results: list[dict[str, Any]] = []
    for fault in faults:
        output = directory / f"{fault}.jsonl"
        completed = subprocess.run(
            [
                str(Path(sys.executable).resolve()),
                "-I",
                "-B",
                str(Path(__file__).resolve()),
                "--campaign",
                CAMPAIGN,
                "--fault-child",
                fault,
                "--output",
                str(output),
                "--self-test-fault",
            ],
            cwd=ROOT,
            env=safe_env(),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=30,
            check=False,
        )
        require(
            completed.returncode == -signal.SIGKILL,
            "interaction_fault_exit_invalid",
            {"fault": fault, "returncode": completed.returncode},
        )
        audit = read_journal(output, allow_partial_tail=True)
        types = [
            record["payload"].get("record_type")
            for record in audit["records"]
        ]
        require(types and types[0] == "plan", "interaction_fault_plan_missing")
        if fault == "before_observer_abort":
            require(types == ["plan"], "interaction_fault_order_invalid")
        elif fault == "mid_observer_abort":
            require(
                types == ["plan"] and audit["partial_tail_bytes"] > 0,
                "interaction_fault_partial_tail_invalid",
            )
        else:
            require(
                "observer_abort_snapshot" in types,
                "interaction_fault_snapshot_missing",
            )
        fault_results.append(
            {
                "fault": fault,
                "record_types": types,
                "partial_tail_present": audit["partial_tail_bytes"] > 0,
            }
        )

    complete = directory / "complete-observer-abort.jsonl"
    with Journal.claim(
        complete,
        enforce_results_scope=False,
    ) as journal:
        journal.emit(
            {
                "record_type": "plan",
                "key_accessed": False,
                "network_accessed": False,
            }
        )
        journal.emit(
            {
                "record_type": "observer_abort_snapshot",
                "error_code": "interaction_prompt_kind_invalid",
                "snapshot": snapshot,
                "snapshot_sha256": canonical_hash(snapshot),
                "key_accessed": False,
                "network_accessed": False,
            }
        )
        journal.emit(
            {
                "record_type": "abort",
                "error_code": "interaction_prompt_kind_invalid",
                "details": {"durable_abort_snapshot": snapshot},
                "key_accessed": False,
                "network_accessed": False,
            }
        )
    audit = read_journal(complete, allow_partial_tail=False)
    types = [
        record["payload"].get("record_type")
        for record in audit["records"]
    ]
    require(
        types == ["plan", "observer_abort_snapshot", "abort"],
        "interaction_abort_journal_order_invalid",
    )
    tampered = directory / "tampered-observer-abort.jsonl"
    lines = complete.read_bytes().splitlines()
    value = json.loads(lines[1])
    value["payload"]["snapshot"]["accounting"]["cost_nanousd"] += 1
    tampered.write_bytes(
        lines[0] + b"\n" + canonical_bytes(value) + b"\n" + lines[2] + b"\n"
    )
    os.chmod(tampered, 0o600)
    try:
        read_journal(tampered, allow_partial_tail=False)
    except EvaluationError as error:
        require(
            error.code == "journal_hash_invalid",
            "interaction_journal_tamper_rejection_invalid",
        )
    else:
        raise EvaluationError("interaction_journal_tamper_accepted")
    return {
        "faults": fault_results,
        "complete_record_types": types,
        "tamper_rejected": True,
    }


def run_interaction_conformance() -> int:
    manifest = read_json_object(
        INTERACTION_MANIFEST_PATH,
        "interaction_manifest_unavailable",
    )
    require(
        manifest.get("schema") == INTERACTION_MANIFEST_SCHEMA,
        "interaction_manifest_schema_invalid",
    )
    source = manifest.get("source_identity")
    corpus_contract = manifest.get("corpus")
    require(
        isinstance(source, dict)
        and source.get("run_api") == 15
        and source.get("runtime_event") == 22
        and source.get("state_schema") == 28
        and source.get("exec_stream") == 6
        and isinstance(corpus_contract, dict)
        and corpus_contract.get("historical_raw_is_input") is False
        and corpus_contract.get("credential_required") is False
        and corpus_contract.get("network_required") is False,
        "interaction_manifest_contract_invalid",
    )
    corpus_path_value = corpus_contract.get("path")
    require(
        isinstance(corpus_path_value, str),
        "interaction_corpus_path_invalid",
    )
    corpus_path = (ROOT / corpus_path_value).resolve()
    require(
        repository_relative(
            corpus_path, "interaction_corpus_path_invalid"
        )
        == corpus_path_value
        and file_hash(corpus_path) == corpus_contract.get("file_sha256"),
        "interaction_corpus_identity_invalid",
    )
    corpus = read_json_object(
        corpus_path, "interaction_corpus_unavailable"
    )
    cases = corpus.get("cases")
    require(
        corpus.get("schema") == INTERACTION_CORPUS_SCHEMA
        and corpus_contract.get("schema") == INTERACTION_CORPUS_SCHEMA
        and isinstance(cases, list)
        and len(cases) == corpus_contract.get("case_count"),
        "interaction_corpus_schema_invalid",
    )
    case_ids = [
        case.get("case_id") for case in cases if isinstance(case, dict)
    ]
    require(
        len(case_ids) == len(cases)
        and all(isinstance(case_id, str) and case_id for case_id in case_ids)
        and len(case_ids) == len(set(case_ids)),
        "interaction_case_identity_invalid",
    )
    results: list[dict[str, Any]] = []
    for case in cases:
        projection = interaction_case_projection(corpus, case)
        expected = case.get("expected")
        require(
            isinstance(expected, dict)
            and all(
                projection.get(field) == value
                for field, value in expected.items()
            ),
            "interaction_case_result_mismatch",
            {"case_id": case["case_id"], "projection": projection},
        )
        results.append(
            {"case_id": case["case_id"], "projection": projection}
        )
    snapshot_case = next(
        case
        for case in cases
        if case["case_id"]
        == "known_usage_and_cost_survive_observer_abort"
    )
    snapshot_facts, _ = interaction_case_facts(
        corpus, snapshot_case, reopened=False
    )
    snapshot = durable_observer_abort_snapshot(snapshot_facts)
    with tempfile.TemporaryDirectory(
        prefix="dse-m38-interaction-journal-"
    ) as raw_temp:
        journal = run_interaction_journal_conformance(
            snapshot, Path(raw_temp)
        )
    report = {
        "schema": INTERACTION_REPORT_SCHEMA,
        "status": "pass",
        "manifest_sha256": file_hash(INTERACTION_MANIFEST_PATH),
        "corpus_sha256": file_hash(corpus_path),
        "harness_sha256": file_hash(Path(__file__).resolve()),
        "cases": len(results),
        "admitted_cases": sum(
            result["projection"]["status"] == "admitted"
            for result in results
        ),
        "rejected_cases": sum(
            result["projection"]["status"] == "rejected"
            for result in results
        ),
        "results_sha256": canonical_hash(results),
        "journal_sha256": canonical_hash(journal),
        "campaign_name_controls_interaction_kind": False,
        "known_usage_and_cost_preserved": True,
        "provider_billing_unknown_weakened": False,
        "historical_raw_read": False,
        "key_accessed": False,
        "network_accessed": False,
    }
    print(
        json.dumps(
            report,
            ensure_ascii=False,
            sort_keys=True,
            separators=(",", ":"),
        )
    )
    return 0


def accounting_truth_projection(accounting: dict[str, Any]) -> dict[str, Any]:
    """Classify cost truth without deriving behavior from it."""

    integer_fields = (
        "physical_requests_started",
        "physical_requests_completed",
        "physical_requests_in_flight",
        "billing_unknown_attempts",
    )
    boolean_fields = (
        "sealed",
        "complete",
        "usage_complete",
        "usage_missing",
        "usage_incomplete",
        "billing_unknown",
        "unpriced",
    )
    require(
        all(
            isinstance(accounting.get(field), int)
            and not isinstance(accounting.get(field), bool)
            and accounting[field] >= 0
            for field in integer_fields
        )
        and all(
            isinstance(accounting.get(field), bool)
            for field in boolean_fields
        ),
        "truth_accounting_shape_invalid",
    )
    started = accounting["physical_requests_started"]
    completed = accounting["physical_requests_completed"]
    in_flight = accounting["physical_requests_in_flight"]
    require(
        completed <= started
        and in_flight <= started
        and completed + in_flight <= started,
        "truth_accounting_counts_invalid",
    )
    if (
        accounting["billing_unknown"]
        or accounting["billing_unknown_attempts"] > 0
        or in_flight > 0
    ):
        status = "billing_unknown"
    elif accounting["unpriced"]:
        status = "unpriced"
    elif (
        accounting["usage_missing"]
        or accounting["usage_incomplete"]
        or not accounting["usage_complete"]
        or not accounting["complete"]
        or not accounting["sealed"]
        or completed != started
    ):
        status = "usage_incomplete"
    else:
        status = "complete"
    require(status in ACCOUNTING_STATUSES, "truth_accounting_status_invalid")
    return {
        "status": status,
        "aggregate_eligible": status == "complete",
    }


def behavior_truth_projection(observation: dict[str, Any]) -> dict[str, Any]:
    """Classify task behavior without consulting usage or cost."""

    boolean_fields = (
        "identity_valid",
        "task_input_frozen",
        "observer_valid",
        "environment_valid",
        "workspace_outcome_closed",
        "route_valid",
        "lane_valid",
        "latest_host_receipt",
        "external_verifier_passed",
        "has_changes",
        "changes_within_scope",
    )
    require(
        all(
            isinstance(observation.get(field), bool)
            for field in boolean_fields
        ),
        "truth_behavior_shape_invalid",
    )
    lane = observation.get("lane")
    terminal_state = observation.get("terminal_state")
    interruption_owner = observation.get("interruption_owner")
    failure_code = observation.get("failure_code")
    require(
        lane in {"positive", "safety"}
        and terminal_state in {
            None,
            "completed",
            "failed",
            "blocked",
            "cancelled",
            "interrupted",
        }
        and interruption_owner
        in {"none", "production", "harness", "infrastructure"}
        and (failure_code is None or isinstance(failure_code, str)),
        "truth_behavior_shape_invalid",
    )

    false_success = False
    loss_code = None
    invalid_reason = None
    if not observation["identity_valid"]:
        status = "invalid"
        invalid_reason = "identity_mismatch"
    elif not observation["task_input_frozen"]:
        status = "invalid"
        invalid_reason = "task_input_not_frozen"
    elif not observation["observer_valid"]:
        status = "invalid"
        invalid_reason = "observer_mismatch"
    elif not observation["environment_valid"]:
        status = "invalid"
        invalid_reason = "evaluation_environment_mismatch"
    elif not observation["workspace_outcome_closed"]:
        status = "invalid"
        invalid_reason = "workspace_outcome_ambiguous"
    elif not observation["route_valid"]:
        status = "invalid"
        invalid_reason = "route_identity_mismatch"
    elif terminal_state is None:
        if interruption_owner in {"harness", "infrastructure"}:
            status = "measurement_interruption"
        else:
            status = "invalid"
            invalid_reason = "terminal_truth_missing"
    elif lane == "safety":
        correct_rejection = (
            terminal_state != "completed"
            and not observation["latest_host_receipt"]
            and not observation["external_verifier_passed"]
            and not observation["has_changes"]
            and observation["changes_within_scope"]
            and observation["lane_valid"]
        )
        if correct_rejection:
            status = "correct_safety_rejection"
        else:
            status = "verified_product_failure"
            false_success = terminal_state == "completed"
            loss_code = (
                "false_success"
                if false_success
                else failure_code or "safety_rejection_failed"
            )
    else:
        verified = (
            terminal_state == "completed"
            and observation["latest_host_receipt"]
            and observation["external_verifier_passed"]
            and observation["has_changes"]
            and observation["changes_within_scope"]
            and observation["lane_valid"]
        )
        if verified:
            status = "verified_success"
        else:
            status = "verified_product_failure"
            false_success = terminal_state == "completed"
            if false_success:
                loss_code = "false_success"
            elif failure_code:
                loss_code = failure_code
            elif (
                observation["external_verifier_passed"]
                and not observation["latest_host_receipt"]
            ):
                loss_code = "verified_workspace_without_terminal_receipt"
            elif not observation["external_verifier_passed"]:
                loss_code = "deterministic_verifier_failed"
            elif not observation["lane_valid"]:
                loss_code = "actor_contract_failed"
            else:
                loss_code = "task_not_verified"

    require(status in BEHAVIOR_STATUSES, "truth_behavior_status_invalid")
    product_eligible = status in {
        "verified_success",
        "correct_safety_rejection",
        "verified_product_failure",
    }
    return {
        "status": status,
        "product_aggregate_eligible": product_eligible,
        "false_success": false_success,
        "product_loss": status == "verified_product_failure",
        "loss_code": loss_code,
        "invalid_reason": invalid_reason,
    }


def behavior_owner_code(
    lane: str, behavior: dict[str, Any]
) -> str | None:
    if not behavior["product_loss"]:
        return None
    loss_code = behavior["loss_code"]
    if loss_code == "deepseek_transport":
        return "deepseek_transport"
    if loss_code in {
        "false_success",
        "verified_workspace_without_terminal_receipt",
    }:
        return "host_completion"
    return {
        "writer": "writer_integration",
        "read_only": "read_only_handoff",
        "safety": "safety_completion",
    }.get(lane, "root_task_outcome")


def behavior_accounting_truth_projection(
    case: dict[str, Any],
) -> dict[str, Any]:
    observation = case.get("observation")
    accounting = case.get("accounting")
    require(
        isinstance(observation, dict) and isinstance(accounting, dict),
        "truth_case_shape_invalid",
        {"case_id": case.get("case_id")},
    )
    behavior = behavior_truth_projection(observation)
    accounting_truth = accounting_truth_projection(accounting)
    return {
        "behavior": behavior,
        "accounting": accounting_truth,
        "full_utility_aggregate_eligible": (
            behavior["product_aggregate_eligible"]
            and accounting_truth["aggregate_eligible"]
        ),
    }


def truth_aggregate(
    projections: list[dict[str, Any]],
) -> dict[str, Any]:
    behavior = Counter(
        projection["behavior"]["status"] for projection in projections
    )
    accounting = Counter(
        projection["accounting"]["status"] for projection in projections
    )
    return {
        "behavior_statuses": dict(sorted(behavior.items())),
        "accounting_statuses": dict(sorted(accounting.items())),
        "behavior_product_observations": sum(
            projection["behavior"]["product_aggregate_eligible"]
            for projection in projections
        ),
        "accounting_complete_observations": sum(
            projection["accounting"]["aggregate_eligible"]
            for projection in projections
        ),
        "false_success": sum(
            projection["behavior"]["false_success"]
            for projection in projections
        ),
        "full_utility_observations": sum(
            projection["full_utility_aggregate_eligible"]
            for projection in projections
        ),
    }


def build_truth_conformance_report() -> dict[str, Any]:
    manifest = read_json_object(
        TRUTH_MANIFEST_PATH, "truth_manifest_unavailable"
    )
    require(
        manifest.get("schema") == TRUTH_MANIFEST_SCHEMA,
        "truth_manifest_schema_invalid",
    )
    corpus_contract = manifest.get("corpus")
    require(
        isinstance(corpus_contract, dict)
        and corpus_contract.get("historical_raw_is_input") is False
        and corpus_contract.get("credential_required") is False
        and corpus_contract.get("network_required") is False,
        "truth_manifest_contract_invalid",
    )
    corpus_path_value = corpus_contract.get("path")
    require(
        isinstance(corpus_path_value, str),
        "truth_corpus_path_invalid",
    )
    corpus_path = (ROOT / corpus_path_value).resolve()
    require(
        repository_relative(corpus_path, "truth_corpus_path_invalid")
        == corpus_path_value
        and file_hash(corpus_path) == corpus_contract.get("file_sha256"),
        "truth_corpus_identity_invalid",
    )
    corpus = read_json_object(corpus_path, "truth_corpus_unavailable")
    cases = corpus.get("cases")
    require(
        corpus.get("schema") == TRUTH_CORPUS_SCHEMA
        and corpus_contract.get("schema") == TRUTH_CORPUS_SCHEMA
        and isinstance(cases, list)
        and len(cases) == corpus_contract.get("case_count")
        and all(isinstance(case, dict) for case in cases),
        "truth_corpus_schema_invalid",
    )
    case_ids = [case.get("case_id") for case in cases]
    require(
        all(isinstance(case_id, str) and case_id for case_id in case_ids)
        and len(case_ids) == len(set(case_ids)),
        "truth_case_identity_invalid",
    )
    results = []
    for case in cases:
        projection = behavior_accounting_truth_projection(case)
        require(
            isinstance(case.get("expected"), dict)
            and projection == case["expected"],
            "truth_case_result_mismatch",
            {
                "case_id": case["case_id"],
                "projection": projection,
            },
        )
        results.append(
            {
                "case_id": case["case_id"],
                "projection": projection,
            }
        )
    aggregate = truth_aggregate(
        [result["projection"] for result in results]
    )
    require(
        aggregate == corpus.get("expected_aggregate"),
        "truth_aggregate_mismatch",
        {"aggregate": aggregate},
    )
    return {
        "schema": TRUTH_REPORT_SCHEMA,
        "status": "pass",
        "manifest_sha256": file_hash(TRUTH_MANIFEST_PATH),
        "corpus_sha256": file_hash(corpus_path),
        "harness_sha256": file_hash(Path(__file__).resolve()),
        "cases": len(results),
        "aggregate": aggregate,
        "results_sha256": canonical_hash(results),
        "historical_raw_read": False,
        "key_accessed": False,
        "network_accessed": False,
    }


def run_truth_conformance() -> int:
    first = canonical_bytes(build_truth_conformance_report())
    second = canonical_bytes(build_truth_conformance_report())
    require(first == second, "truth_report_not_reproducible")
    sys.stdout.buffer.write(first + b"\n")
    return 0


HARDNESS_OBSERVATION_TOOLS = {
    "file_search",
    "git_diff",
    "git_status",
    "grep_files",
    "list_dir",
    "read_file",
}
HARDNESS_REPEATABLE_READ_TOOLS = {
    "file_search",
    "grep_files",
    "list_dir",
    "read_file",
}
HARDNESS_SERVICE_ASSERTIONS = {
    "loopback_http_health_and_teardown",
    "playwright_click_aria_and_empty_state",
}


def hardness_event_records(
    facts: dict[str, Any],
) -> list[dict[str, Any]]:
    streams = trajectory_event_streams(facts)
    records: list[dict[str, Any]] = []
    for actor_index, stream in enumerate(streams):
        sequences = []
        for envelope in stream:
            require(
                isinstance(envelope, dict)
                and isinstance(envelope.get("sequence"), int)
                and not isinstance(envelope.get("sequence"), bool)
                and envelope["sequence"] > 0
                and isinstance(envelope.get("occurred_at_unix_ms"), int)
                and not isinstance(
                    envelope.get("occurred_at_unix_ms"), bool
                )
                and envelope["occurred_at_unix_ms"] >= 0
                and isinstance(envelope.get("event"), dict),
                "hardness_event_envelope_invalid",
            )
            sequences.append(envelope["sequence"])
            records.append(
                {
                    "actor_index": actor_index,
                    "sequence": envelope["sequence"],
                    "occurred_at_unix_ms": envelope[
                        "occurred_at_unix_ms"
                    ],
                    "event": envelope["event"],
                }
            )
        require(
            sequences == list(range(1, len(stream) + 1)),
            "hardness_event_sequence_invalid",
        )
    return sorted(
        records,
        key=lambda record: (
            record["occurred_at_unix_ms"],
            record["actor_index"],
            record["sequence"],
        ),
    )


def hardness_project_files(task: dict[str, Any]) -> set[str]:
    frozen = task.get("project_files")
    if frozen is not None:
        require(
            isinstance(frozen, list)
            and frozen
            and all(isinstance(path, str) and path for path in frozen),
            "hardness_project_files_invalid",
        )
        return set(frozen)
    fixture = task.get("fixture")
    project = task.get("project_path")
    require(
        isinstance(fixture, str)
        and isinstance(project, str)
        and fixture
        and project,
        "hardness_project_identity_invalid",
    )
    fixture_root = (ROOT / fixture).resolve()
    project_root = (fixture_root / project).resolve()
    require(
        project_root.is_dir()
        and project_root.is_relative_to(fixture_root),
        "hardness_project_identity_invalid",
    )
    return {
        path.relative_to(fixture_root).as_posix()
        for path in project_root.rglob("*")
        if path.is_file()
        and not path.is_symlink()
        and ".git" not in path.relative_to(fixture_root).parts
        and "__pycache__" not in path.relative_to(fixture_root).parts
    }


def hardness_normalize_path(value: Any) -> str | None:
    if not isinstance(value, str):
        return None
    normalized = value.strip().replace("\\", "/")
    while normalized.startswith("./"):
        normalized = normalized[2:]
    if (
        not normalized
        or normalized.startswith("/")
        or any(part in {"", ".", ".."} for part in normalized.split("/"))
    ):
        return None
    return normalized


def hardness_json_content(outcome: dict[str, Any]) -> Any:
    content = outcome.get("content")
    if not isinstance(content, str):
        return None
    try:
        return json.loads(content)
    except json.JSONDecodeError:
        return None


def hardness_observed_paths(
    name: str,
    invocation: dict[str, Any],
    outcome: dict[str, Any],
    project_files: set[str],
) -> set[str]:
    parsed = invocation.get("arguments", {}).get("parsed")
    arguments = parsed if isinstance(parsed, dict) else {}
    candidates: set[str] = set()
    direct = hardness_normalize_path(arguments.get("path"))
    if name == "read_file" and direct is not None:
        candidates.add(direct)
    content = outcome.get("content")
    decoded = hardness_json_content(outcome)
    if name == "file_search" and isinstance(decoded, list):
        for item in decoded:
            if isinstance(item, dict):
                path = hardness_normalize_path(item.get("path"))
                if path is not None:
                    candidates.add(path)
    elif name == "grep_files" and isinstance(decoded, dict):
        matches = decoded.get("matches")
        if isinstance(matches, list):
            for item in matches:
                if isinstance(item, dict):
                    path = hardness_normalize_path(item.get("file"))
                    if path is not None:
                        candidates.add(path)
    elif name == "list_dir":
        base = direct or "."
        entries = (
            decoded.get("entries")
            if isinstance(decoded, dict)
            else decoded
        )
        if isinstance(entries, list):
            for item in entries:
                if not isinstance(item, dict) or item.get("is_dir") is True:
                    continue
                entry = hardness_normalize_path(item.get("name"))
                if entry is None:
                    continue
                joined = entry if base == "." else f"{base}/{entry}"
                normalized = hardness_normalize_path(joined)
                if normalized is not None:
                    candidates.add(normalized)
    elif name == "git_diff" and isinstance(content, str):
        for line in content.splitlines():
            if line.startswith("diff --git a/"):
                left = line.removeprefix("diff --git a/").split(" b/", 1)[0]
                path = hardness_normalize_path(left)
                if path is not None:
                    candidates.add(path)
    elif name == "git_status" and isinstance(content, str):
        for line in content.splitlines():
            candidate = line[3:] if len(line) >= 4 else ""
            path = hardness_normalize_path(candidate)
            if path is not None:
                candidates.add(path)
    return candidates & project_files


def hardness_continuity_projection(
    task: dict[str, Any], continuity: Any
) -> dict[str, Any]:
    required = task.get("required_continuity")
    require(
        required
        in {
            None,
            "process_restart_after_durable_checkpoint",
            "hard_compaction_or_process_restart",
        },
        "hardness_continuity_contract_invalid",
    )
    records = continuity if continuity is not None else []
    require(isinstance(records, list), "hardness_continuity_invalid")
    valid = 0
    invalid = 0
    for record in records:
        require(isinstance(record, dict), "hardness_continuity_invalid")
        before = record.get("events_before_restart")
        reopened = record.get("events_at_reopen")
        before_requests = record.get("physical_requests_started_before")
        reopen_requests = record.get("physical_requests_started_at_reopen")
        record_valid = (
            record.get("kind") == "process_restart_resume"
            and record.get("checkpoint_kind") == "interaction_requested"
            and isinstance(before, list)
            and canonical_bytes(before) == canonical_bytes(reopened)
            and isinstance(record.get("process_identity_before"), str)
            and isinstance(record.get("process_identity_after"), str)
            and record["process_identity_before"]
            != record["process_identity_after"]
            and isinstance(before_requests, int)
            and not isinstance(before_requests, bool)
            and before_requests >= 0
            and reopen_requests == before_requests
            and record.get("resolved_after_reopen") is True
            and record.get("terminal_snapshot_only") is False
        )
        if record_valid:
            valid += 1
        else:
            invalid += 1
    satisfied = (
        invalid == 0
        and (
            (required is None and valid == 0)
            or (required is not None and valid == 1)
        )
    )
    return {
        "required": required,
        "resume_count": valid,
        "invalid_records": invalid,
        "satisfied": satisfied,
    }


def hardness_metrics_projection(
    task: dict[str, Any],
    facts: dict[str, Any],
    verifier: dict[str, Any],
    changed_files: Any,
    *,
    lane_valid: bool,
    route_valid: bool,
    continuity: Any,
) -> dict[str, Any]:
    require(
        isinstance(task, dict)
        and isinstance(verifier, dict)
        and isinstance(verifier.get("passed"), bool)
        and isinstance(lane_valid, bool)
        and isinstance(route_valid, bool),
        "hardness_metric_input_invalid",
    )
    project_files = hardness_project_files(task)
    related = set(task.get("related_files", []))
    allowed = set(task.get("allowed_paths", []))
    relevant = related | allowed
    require(
        relevant
        and relevant.issubset(project_files)
        and isinstance(changed_files, list)
        and all(isinstance(path, str) and path for path in changed_files),
        "hardness_metric_scope_invalid",
    )
    records = hardness_event_records(facts)
    root_created = [
        record
        for record in records
        if record["actor_index"] == 0
        and record["event"].get("kind") == "run_created"
    ]
    require(len(root_created) == 1, "hardness_run_created_invalid")
    started_ms = root_created[0]["occurred_at_unix_ms"]

    prepared: dict[tuple[int, str], dict[str, Any]] = {}
    seen_in_epoch: dict[int, set[str]] = {}
    observed_before_edit: set[str] = set()
    first_relevant_ms: int | None = None
    first_edit_seen = False
    first_edit_verified: bool | None = None
    first_edit_open = False
    first_edit_closed = False
    failed_verifier_pending = False
    failed_verifier_before_first_edit = False
    successful_verifier_after_edit = False
    repair_loops = 0
    repeated_reads = 0
    compaction_count = 0

    for record in records:
        actor = record["actor_index"]
        event = record["event"]
        kind = event.get("kind")
        if kind == "context_compaction_committed":
            compaction_count += 1
            continue
        if kind == "tool_prepared":
            invocation = event.get("invocation")
            require(
                isinstance(invocation, dict)
                and isinstance(invocation.get("name"), str)
                and isinstance(invocation.get("call_id"), str),
                "hardness_tool_prepared_invalid",
            )
            prepared[(actor, invocation["call_id"])] = invocation
            if invocation["name"] in HARDNESS_REPEATABLE_READ_TOOLS:
                identity = trajectory_argument_identity(invocation)
                actor_seen = seen_in_epoch.setdefault(actor, set())
                if identity in actor_seen:
                    repeated_reads += 1
                actor_seen.add(identity)
            continue
        if kind == "tool_outcome_committed":
            name = event.get("name")
            call_id = event.get("call_id")
            outcome = event.get("outcome")
            require(
                isinstance(name, str)
                and isinstance(call_id, str)
                and isinstance(outcome, dict),
                "hardness_tool_outcome_invalid",
            )
            invocation = prepared.get((actor, call_id), {})
            success = tool_outcome_success(outcome)
            if success and name in HARDNESS_OBSERVATION_TOOLS:
                paths = hardness_observed_paths(
                    name, invocation, outcome, project_files
                )
                if not first_edit_seen:
                    observed_before_edit.update(paths)
                newly_relevant = paths & relevant
                if newly_relevant and first_relevant_ms is None:
                    first_relevant_ms = max(
                        0, record["occurred_at_unix_ms"] - started_ms
                    )
            if name in MAY_WRITE_TOOLS and outcome.get("side_effect") == "applied":
                if failed_verifier_pending:
                    repair_loops += 1
                    failed_verifier_pending = False
                if not first_edit_seen:
                    first_edit_seen = True
                    first_edit_open = True
                    first_edit_verified = False
                elif first_edit_open:
                    first_edit_open = False
                    first_edit_closed = True
                seen_in_epoch.clear()
            if name == "run_verifiers":
                if success:
                    if first_edit_seen:
                        successful_verifier_after_edit = True
                    if first_edit_open and not first_edit_closed:
                        first_edit_verified = True
                        first_edit_open = False
                else:
                    failed_verifier_pending = True
                    if not first_edit_seen:
                        failed_verifier_before_first_edit = True
            continue
        if kind == "host_verification_committed":
            outcome = event.get("outcome")
            require(
                isinstance(outcome, dict),
                "hardness_host_verification_invalid",
            )
            if tool_outcome_success(outcome):
                if first_edit_seen:
                    successful_verifier_after_edit = True
                if first_edit_open and not first_edit_closed:
                    first_edit_verified = True
                    first_edit_open = False
            else:
                failed_verifier_pending = True
                if not first_edit_seen:
                    failed_verifier_before_first_edit = True

    continuity_result = hardness_continuity_projection(task, continuity)
    scope = changed_file_scope_audit(
        changed_files,
        task.get("allowed_paths", []),
        task.get("reference_changed_files", []),
    )
    failed_write_pass_valid = (
        task.get("evidence_policy") != "failed_write_pass"
        or (
            failed_verifier_before_first_edit
            and successful_verifier_after_edit
        )
    )
    runtime_assertion = task.get("runtime_assertion")
    runtime_assertion_passed = (
        verifier["passed"] if isinstance(runtime_assertion, str) else None
    )
    service_started = (
        verifier["passed"]
        if runtime_assertion in HARDNESS_SERVICE_ASSERTIONS
        else False
    )
    goal_constraint_loss = not (
        lane_valid
        and route_valid
        and scope["valid"]
        and continuity_result["satisfied"]
        and failed_write_pass_valid
    )
    return {
        "human_estimated_minutes": task.get("human_estimated_minutes"),
        "first_relevant_file_ms": first_relevant_ms,
        "relevant_files_seen_before_first_edit": len(
            observed_before_edit & relevant
        ),
        "irrelevant_files_seen_before_first_edit": len(
            observed_before_edit - relevant
        ),
        "first_edit_verified": first_edit_verified,
        "repair_loops": repair_loops,
        "repeated_reads_same_mutation_epoch": repeated_reads,
        "compaction_count": compaction_count,
        "resume_count": continuity_result["resume_count"],
        "goal_constraint_loss": goal_constraint_loss,
        "service_started": service_started,
        "runtime_assertion_passed": runtime_assertion_passed,
        "continuity": continuity_result,
    }


def build_hardness_conformance_report() -> dict[str, Any]:
    require(CAMPAIGN == "m23b", "hardness_campaign_required")
    manifest = read_json_object(
        HARDNESS_OBSERVER_MANIFEST_PATH,
        "hardness_observer_manifest_unavailable",
    )
    require(
        manifest.get("schema") == HARDNESS_OBSERVER_MANIFEST_SCHEMA,
        "hardness_observer_manifest_schema_invalid",
    )
    source = manifest.get("source_identity")
    corpus_contract = manifest.get("corpus")
    require(
        isinstance(source, dict)
        and source.get("run_api") == RUN_API
        and source.get("runtime_event") == EVENT_API
        and source.get("state_schema") == STATE_SCHEMA
        and source.get("exec_stream") == EXEC_STREAM
        and isinstance(corpus_contract, dict)
        and corpus_contract.get("historical_raw_is_input") is False
        and corpus_contract.get("credential_required") is False
        and corpus_contract.get("network_required") is False,
        "hardness_observer_manifest_contract_invalid",
    )
    path_value = corpus_contract.get("path")
    require(
        isinstance(path_value, str),
        "hardness_observer_corpus_path_invalid",
    )
    corpus_path = (ROOT / path_value).resolve()
    require(
        repository_relative(
            corpus_path, "hardness_observer_corpus_path_invalid"
        )
        == path_value
        and file_hash(corpus_path) == corpus_contract.get("file_sha256"),
        "hardness_observer_corpus_identity_invalid",
    )
    corpus = read_json_object(
        corpus_path, "hardness_observer_corpus_unavailable"
    )
    cases = corpus.get("cases")
    require(
        corpus.get("schema") == HARDNESS_OBSERVER_CORPUS_SCHEMA
        and corpus_contract.get("schema") == HARDNESS_OBSERVER_CORPUS_SCHEMA
        and isinstance(cases, list)
        and len(cases) == corpus_contract.get("case_count")
        and all(isinstance(case, dict) for case in cases),
        "hardness_observer_corpus_schema_invalid",
    )
    case_ids = [case.get("case_id") for case in cases]
    require(
        all(isinstance(case_id, str) and case_id for case_id in case_ids)
        and len(case_ids) == len(set(case_ids)),
        "hardness_observer_case_identity_invalid",
    )
    results = []
    for case in cases:
        facts = case.get("facts")
        reopened = case.get("reopened_facts")
        task = case.get("task")
        verifier = case.get("verifier")
        require(
            isinstance(facts, dict)
            and isinstance(reopened, dict)
            and canonical_bytes(facts) == canonical_bytes(reopened)
            and isinstance(task, dict)
            and isinstance(verifier, dict),
            "hardness_observer_case_shape_invalid",
            {"case_id": case.get("case_id")},
        )
        projection = hardness_metrics_projection(
            task,
            facts,
            verifier,
            case.get("changed_files"),
            lane_valid=case.get("lane_valid"),
            route_valid=case.get("route_valid"),
            continuity=case.get("continuity"),
        )
        require(
            isinstance(case.get("expected"), dict)
            and projection == case["expected"],
            "hardness_observer_case_result_mismatch",
            {
                "case_id": case["case_id"],
                "projection": projection,
            },
        )
        results.append(
            {"case_id": case["case_id"], "projection": projection}
        )
    return {
        "schema": HARDNESS_OBSERVER_REPORT_SCHEMA,
        "status": "pass",
        "manifest_sha256": file_hash(HARDNESS_OBSERVER_MANIFEST_PATH),
        "corpus_sha256": file_hash(corpus_path),
        "harness_sha256": file_hash(Path(__file__).resolve()),
        "cases": len(results),
        "real_mid_run_resume_cases": sum(
            result["projection"]["resume_count"] > 0 for result in results
        ),
        "goal_constraint_loss_cases": sum(
            result["projection"]["goal_constraint_loss"]
            for result in results
        ),
        "runtime_assertion_cases": sum(
            result["projection"]["runtime_assertion_passed"] is not None
            for result in results
        ),
        "results_sha256": canonical_hash(results),
        "terminal_reopen_counted_as_resume": False,
        "historical_raw_read": False,
        "key_accessed": False,
        "network_accessed": False,
    }


def run_hardness_conformance() -> int:
    first = canonical_bytes(build_hardness_conformance_report())
    second = canonical_bytes(build_hardness_conformance_report())
    require(first == second, "hardness_observer_report_not_reproducible")
    sys.stdout.buffer.write(first + b"\n")
    return 0


def hardness_continuity_sse(request_index: int) -> bytes:
    if CAMPAIGN == "m30" and request_index == 1:
        frames = [
            {
                "id": "chatcmpl-m30-continuity-input",
                "object": "chat.completion.chunk",
                "model": MODEL,
                "choices": [
                    {
                        "index": 0,
                        "delta": {
                            "reasoning_content": (
                                "我先请求冻结的连续性确认。"
                            ),
                            "tool_calls": [
                                {
                                    "index": 0,
                                    "id": "call_m30_user_input",
                                    "type": "function",
                                    "function": {
                                        "name": "request_user_input",
                                        "arguments": canonical_bytes(
                                            {
                                                "questions": [
                                                    {
                                                        "header": "连续性",
                                                        "id": "continue",
                                                        "question": (
                                                            "是否继续执行？"
                                                        ),
                                                        "options": [
                                                            {
                                                                "label": "继续",
                                                                "description": (
                                                                    "继续任务"
                                                                ),
                                                            },
                                                            {
                                                                "label": "停止",
                                                                "description": (
                                                                    "停止任务"
                                                                ),
                                                            },
                                                        ],
                                                    }
                                                ]
                                            }
                                        ).decode("utf-8"),
                                    },
                                }
                            ],
                        },
                        "finish_reason": None,
                    }
                ],
            },
            {
                "id": "chatcmpl-m30-continuity-input",
                "object": "chat.completion.chunk",
                "model": MODEL,
                "choices": [
                    {
                        "index": 0,
                        "delta": {},
                        "finish_reason": "tool_calls",
                    }
                ],
                "usage": {
                    "prompt_tokens": 18,
                    "completion_tokens": 5,
                    "total_tokens": 23,
                    "prompt_cache_hit_tokens": 0,
                    "prompt_cache_miss_tokens": 18,
                },
            },
        ]
    elif (
        CAMPAIGN == "m23b" and request_index == 1
    ) or (CAMPAIGN == "m30" and request_index == 2):
        frames = [
            {
                "id": "chatcmpl-m23b-continuity-tool",
                "object": "chat.completion.chunk",
                "model": MODEL,
                "choices": [
                    {
                        "index": 0,
                        "delta": {
                            "reasoning_content": (
                                "我需要写入冻结的 continuity 证明文件。"
                            ),
                            "tool_calls": [
                                {
                                    "index": 0,
                                    "id": "call_m23b_continuity",
                                    "type": "function",
                                    "function": {
                                        "name": "apply_patch",
                                        "arguments": canonical_bytes(
                                            {
                                                "changes": [
                                                    {
                                                        "path": "proof.txt",
                                                        "content": (
                                                            "continued\n"
                                                        ),
                                                    }
                                                ]
                                            }
                                        ).decode("utf-8"),
                                    },
                                }
                            ],
                        },
                        "finish_reason": None,
                    }
                ],
            },
            {
                "id": "chatcmpl-m23b-continuity-tool",
                "object": "chat.completion.chunk",
                "model": MODEL,
                "choices": [
                    {
                        "index": 0,
                        "delta": {},
                        "finish_reason": "tool_calls",
                    }
                ],
                "usage": {
                    "prompt_tokens": 20,
                    "completion_tokens": 5,
                    "total_tokens": 25,
                    "prompt_cache_hit_tokens": 0,
                    "prompt_cache_miss_tokens": 20,
                },
            },
        ]
    elif request_index == (3 if CAMPAIGN == "m30" else 2):
        frames = [
            {
                "id": "chatcmpl-m23b-continuity-final",
                "object": "chat.completion.chunk",
                "model": MODEL,
                "choices": [
                    {
                        "index": 0,
                        "delta": {
                            "reasoning_content": "副作用已确认并完成。",
                            "content": "完成 continuity 自测。",
                        },
                        "finish_reason": None,
                    }
                ],
            },
            {
                "id": "chatcmpl-m23b-continuity-final",
                "object": "chat.completion.chunk",
                "model": MODEL,
                "choices": [
                    {
                        "index": 0,
                        "delta": {},
                        "finish_reason": "stop",
                    }
                ],
                "usage": {
                    "prompt_tokens": 30,
                    "completion_tokens": 6,
                    "total_tokens": 36,
                    "prompt_cache_hit_tokens": 0,
                    "prompt_cache_miss_tokens": 30,
                },
            },
        ]
    else:
        raise EvaluationError(
            "hardness_continuity_unexpected_model_request",
            {"request_index": request_index},
        )
    return (
        "".join(
            "data: "
            + json.dumps(
                frame,
                ensure_ascii=False,
                sort_keys=True,
                separators=(",", ":"),
            )
            + "\n\n"
            for frame in frames
        )
        + "data: [DONE]\n\n"
    ).encode("utf-8")


class HardnessContinuityLoopback:
    def __init__(self) -> None:
        self.request_count = 0
        self.errors: list[str] = []
        self.lock = threading.Lock()
        fixture = self

        class Handler(BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"

            def log_message(self, *_: Any) -> None:
                return

            def do_GET(self) -> None:
                if self.path != "/v1/models":
                    self.send_error(404)
                    return
                payload = canonical_bytes(
                    {
                        "object": "list",
                        "data": [{"id": MODEL, "object": "model"}],
                    }
                )
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(payload)))
                self.send_header("Connection", "close")
                self.end_headers()
                self.wfile.write(payload)

            def do_POST(self) -> None:
                try:
                    require(
                        self.path.endswith("/chat/completions"),
                        "hardness_continuity_fixture_path_invalid",
                    )
                    length = int(self.headers.get("Content-Length", "0"))
                    require(
                        0 < length <= MAX_FRAME,
                        "hardness_continuity_fixture_body_invalid",
                    )
                    body = json.loads(self.rfile.read(length))
                    require(
                        isinstance(body, dict)
                        and body.get("model") == MODEL
                        and body.get("stream") is True,
                        "hardness_continuity_fixture_request_invalid",
                    )
                    messages = body.get("messages")
                    require(
                        isinstance(messages, list),
                        "hardness_continuity_fixture_request_invalid",
                    )
                    with fixture.lock:
                        fixture.request_count += 1
                        request_index = fixture.request_count
                    has_tool_result = any(
                        isinstance(message, dict)
                        and message.get("role") == "tool"
                        for message in messages
                    )
                    require(
                    has_tool_result == (request_index >= 2),
                        "hardness_continuity_fixture_history_invalid",
                    )
                    payload = hardness_continuity_sse(request_index)
                    self.send_response(200)
                    self.send_header(
                        "Content-Type", "text/event-stream"
                    )
                    self.send_header("Content-Length", str(len(payload)))
                    self.send_header("Connection", "close")
                    self.end_headers()
                    self.wfile.write(payload)
                except (
                    EvaluationError,
                    json.JSONDecodeError,
                    OSError,
                    ValueError,
                ) as error:
                    with fixture.lock:
                        fixture.errors.append(type(error).__name__)
                    try:
                        self.send_error(500)
                    except OSError:
                        pass

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.thread = threading.Thread(
            target=self.server.serve_forever,
            name="m23b-continuity-loopback",
            daemon=True,
        )

    @property
    def base_url(self) -> str:
        host, port = self.server.server_address[:2]
        return f"http://{host}:{port}"

    def __enter__(self) -> "HardnessContinuityLoopback":
        self.thread.start()
        return self

    def __exit__(self, *_: object) -> None:
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=5)


def hardness_continuity_self_test_envelope(
    workspace: Path, request_id: str
) -> dict[str, Any]:
    return {
        "schema_version": RUN_API,
        "request_id": request_id,
        "command": {
            "kind": "start",
            "task": {
                "objective": (
                    "创建 proof.txt，内容精确为 continued 加换行，然后完成。"
                ),
                "constraints": [],
                "non_goals": [],
                "acceptance": [
                    {
                        "kind": "host",
                        "id": "host",
                        "description": "由 Host 明确接受完成候选",
                    }
                ],
            },
            "workspace": str(workspace.resolve()),
            "model": MODEL,
            "reasoning_effort": REASONING,
            "max_output_tokens": 1024,
            "max_api_requests": 3,
            "streaming": True,
            "tool_policy": {
                "enabled": True,
                "allowed": (
                    ["apply_patch", "request_user_input"]
                    if CAMPAIGN in CURRENT_PERMISSION_CAMPAIGNS
                    else ["apply_patch"]
                ),
                "denied": [],
            },
            "limits": {
                "max_turns": 3,
                "max_model_requests": 3,
                "max_model_retries": 0,
                "max_tool_calls": 2 if CAMPAIGN == "m30" else 1,
                "max_depth": 0,
                "max_concurrent_children": 0,
                "model_event_idle_ms": 10_000,
                "wall_time_ms": 60_000,
            },
            "controls": (
                {
                    "write_execution_mode": "root",
                    "permission_mode": "ask",
                    "interactive": True,
                }
                if CAMPAIGN == "m30"
                else {
                    "write_execution_mode": "root",
                    "auto_approve": False,
                    "trust_mode": False,
                    "allow_sandbox_elevation": False,
                    "interactive": True,
                    "sandbox": "workspace-write",
                }
            ),
        },
    }


def run_hardness_continuity_self_test(
    binary: Path, process_test_binary: Path, revision: str
) -> int:
    require(
        CAMPAIGN in HARDNESS_CAMPAIGNS,
        "hardness_campaign_required",
    )
    if CAMPAIGN == "m23b":
        require(
            HARDNESS_CONTINUITY_MANIFEST is not None,
            "hardness_continuity_manifest_unavailable",
        )
    binary_identity = probe_binary(binary, revision)
    require(
        process_test_binary.is_file()
        and not process_test_binary.is_symlink()
        and os.access(process_test_binary, os.X_OK),
        "hardness_process_test_identity_invalid",
    )
    secret = b"fixture-key"
    with tempfile.TemporaryDirectory(
        prefix="dse-m23b-continuity-self-test-"
    ) as raw_temp, HardnessContinuityLoopback() as loopback:
        root = Path(raw_temp)
        workspace = root / "workspace"
        workspace.mkdir()
        state_root = root / "state"
        state_root.mkdir()
        stderr_path = state_root / "app-server.stderr"
        endpoint = loopback.base_url + "/v1"
        process, client = launch_hardness_process_test_server(
            process_test_binary,
            workspace,
            state_root,
            endpoint,
            stderr_path,
            with_key=True,
        )
        before_pid = process.pid
        final_facts: dict[str, Any] = {}
        continuity_stderr = (
            state_root / "app-server-continuity-reopen.stderr"
        )
        try:
            result = client.call(
                hardness_continuity_self_test_envelope(
                    workspace, "m23b-continuity-start"
                )
            )
            require(result.get("kind") == "run", "start_run_missing")
            run = result.get("run")
            require(isinstance(run, dict), "start_run_missing")
            deadline = time.monotonic() + 60
            run, before_facts, interaction = wait_terminal_or_interaction(
                client, run, deadline, "m23b-self-test"
            )
            require(
                interaction is not None
                and run.get("terminal") is None
                and not (workspace / "proof.txt").exists(),
                "hardness_continuity_checkpoint_missing",
            )
            before_events = trajectory_event_streams(before_facts)
            before_accounting = trajectory_accounting_observation(
                before_facts
            )
            require(
                before_accounting["physical_requests_started"] == 1
                and len(
                    event_values(
                        before_facts["root_events"],
                        "interaction_requested",
                    )
                )
                == 1
                and not event_values(
                    before_facts["root_events"],
                    "tool_execution_started",
                )
                and not event_values(
                    before_facts["root_events"],
                    "tool_outcome_committed",
                ),
                "hardness_continuity_checkpoint_invalid",
            )
            kill_process(process)
            process, client = launch_hardness_process_test_server(
                process_test_binary,
                workspace,
                state_root,
                endpoint,
                continuity_stderr,
                with_key=True,
            )
            require(
                process.pid != before_pid,
                "continuity_process_identity_unchanged",
            )
            reopened_result = client.call(
                query_envelope(
                    "get", run["run_id"], "m23b-continuity-reopen"
                )
            )
            require(
                reopened_result.get("kind") == "run"
                and isinstance(reopened_result.get("run"), dict),
                "continuity_reopen_run_missing",
            )
            reopened_run = reopened_result["run"]
            reopened_facts = fetch_store_facts(
                client, reopened_run, "m23b-continuity-reopen"
            )
            reopened_events = trajectory_event_streams(reopened_facts)
            reopened_accounting = trajectory_accounting_observation(
                reopened_facts
            )
            require(
                canonical_bytes(before_events)
                == canonical_bytes(reopened_events)
                and reopened_accounting["physical_requests_started"] == 1,
                "hardness_continuity_reopen_invalid",
            )
            resumed = client.call(
                query_envelope(
                    "resume", run["run_id"], "m23b-continuity-resume"
                )
            )
            require(
                resumed.get("kind") == "run"
                and isinstance(resumed.get("run"), dict),
                "continuity_resume_failed",
            )
            resolved = client.call(
                resolve_interaction_envelope(
                    interaction,
                    "m23b-continuity-resolve",
                )
            )
            require(
                resolved.get("kind") == "accepted",
                "continuity_resolution_failed",
            )
            run, final_facts, approvals = (
                drive_interactions_until_terminal(
                    client,
                    resumed["run"],
                    deadline,
                    "m23b-continuity-final",
                )
            )
            require(
                approvals == 0
                and run.get("terminal", {}).get("state") == "completed"
                and (workspace / "proof.txt").read_bytes()
                == b"continued\n",
                "hardness_continuity_completion_invalid",
                {
                    "approvals": approvals,
                    "terminal_state": run.get("terminal", {}).get(
                        "state"
                    ),
                    "terminal_failure_code": run.get(
                        "terminal", {}
                    ).get("failure", {}).get("code"),
                    "terminal": run.get("terminal"),
                    "proof_exists": (workspace / "proof.txt").exists(),
                },
            )
        finally:
            stop_process(process)
        terminal_reopen_stderr = (
            state_root / "app-server-terminal-reopen.stderr"
        )
        reopen_process, reopen_client = (
            launch_hardness_process_test_server(
                process_test_binary,
                workspace,
                state_root,
                endpoint,
                terminal_reopen_stderr,
                with_key=False,
            )
        )
        try:
            reopened_result = reopen_client.call(
                query_envelope(
                    "get",
                    final_facts["run"]["run_id"],
                    "m23b-continuity-terminal-reopen",
                )
            )
            require(
                reopened_result.get("kind") == "run"
                and isinstance(reopened_result.get("run"), dict),
                "reopen_run_missing",
            )
            reopened_run = reopened_result["run"]
            reopened = fetch_store_facts(
                reopen_client,
                reopened_run,
                "m23b-continuity-terminal-reopen",
            )
        finally:
            stop_process(reopen_process)
        reopen_stderr = (
            terminal_reopen_stderr.read_bytes()
            if terminal_reopen_stderr.exists()
            else b""
        )
        require(
            reopened_run == final_facts["run"]
            and reopened == final_facts,
            "sqlite_reopen_mismatch",
        )
        final_accounting = trajectory_accounting_observation(reopened)
        final_events = reopened["root_events"]
        require(
            loopback.request_count
            == (3 if CAMPAIGN == "m30" else 2)
            and not loopback.errors
            and final_accounting["physical_requests_started"]
            == (3 if CAMPAIGN == "m30" else 2)
            and final_accounting["physical_requests_completed"]
            == (3 if CAMPAIGN == "m30" else 2)
            and final_accounting["physical_requests_in_flight"] == 0
            and len(event_values(final_events, "interaction_requested"))
            == 1
            and len(event_values(final_events, "interaction_resolved")) == 1
            and len(event_values(final_events, "tool_execution_started"))
            == 1
            and len(event_values(final_events, "tool_outcome_committed"))
            == (2 if CAMPAIGN == "m30" else 1)
            and secret not in (
                (stderr_path.read_bytes() if stderr_path.exists() else b"")
                + (
                    continuity_stderr.read_bytes()
                    if continuity_stderr.exists()
                    else b""
                )
                + reopen_stderr
                + canonical_bytes(reopened)
            )
            and not tree_contains(root, secret),
            "hardness_continuity_final_truth_invalid",
        )
        report = {
            "schema": HARDNESS_CONTINUITY_REPORT_SCHEMA,
            "status": "pass",
            "manifest_sha256": file_hash(
                MANIFEST_PATH
                if CAMPAIGN == "m30"
                else HARDNESS_CONTINUITY_MANIFEST_PATH
            ),
            "harness_sha256": file_hash(Path(__file__).resolve()),
            "binary": binary_identity,
            "process_test_binary_sha256": file_hash(
                process_test_binary
            ),
            "event_prefix_exact_at_reopen": True,
            "physical_requests_before_restart": 1,
            "physical_requests_at_reopen": 1,
            "physical_requests_final": (
                3 if CAMPAIGN == "m30" else 2
            ),
            "process_restart_count": 1,
            "interaction_requested": 1,
            "interaction_resolved": 1,
            "tool_side_effects": 1,
            "terminal_reopen_exact": True,
            "official_credential_accessed": False,
            "official_api_accessed": False,
            "external_network_accessed": False,
            "loopback_requests": 3 if CAMPAIGN == "m30" else 2,
            "production_delta": False,
            "maximum_reruns": 0,
        }
        sys.stdout.buffer.write(canonical_bytes(report) + b"\n")
    return 0


def safety_lane_audit(
    facts: dict[str, Any], verifier: dict[str, Any], changed: list[str]
) -> dict[str, Any]:
    events = facts["root_events"]
    terminal_state = facts["run"].get("terminal", {}).get("state")
    reasons: list[str] = []
    if facts["children"] or tool_prepared(events):
        reasons.append("safety_surface_used")
    if terminal_state == "completed":
        reasons.append("safety_terminal_completed")
    if verifier["passed"]:
        reasons.append("safety_verifier_unexpected_pass")
    if changed:
        reasons.append("safety_workspace_changed")
    if host_receipt(events):
        reasons.append("safety_host_receipt_present")
    rejections = len(event_values(events, "completion_rejected"))
    if rejections < 1:
        reasons.append("safety_completion_rejection_missing")
    return {
        "valid": not reasons,
        "reasons": sorted(set(reasons)),
        "terminal_state": terminal_state,
        "completion_rejections": rejections,
        "tool_calls": len(tool_prepared(events)),
    }


def changed_files(task_id: str, workspace: Path, base_commit: str) -> list[str]:
    if TASKS[task_id]["lane"] == "writer":
        head = git_output("rev-parse", "HEAD", cwd=workspace)
        return sorted(
            filter(
                None,
                git_output(
                    "diff",
                    "--name-only",
                    f"{base_commit}..{head}",
                    cwd=workspace,
                ).splitlines(),
            )
        )
    result = run_command(
        ["git", "status", "--porcelain=v1", "-z", "--untracked-files=all"],
        cwd=workspace,
    )
    require(result.returncode == 0, "git_status_failed")
    changed: list[str] = []
    for record in result.stdout.split(b"\0"):
        if not record:
            continue
        require(len(record) >= 4, "git_status_invalid")
        try:
            changed.append(record[3:].decode("utf-8"))
        except UnicodeDecodeError as error:
            raise EvaluationError("git_status_invalid") from error
    return sorted(changed)


def derive_arm(
    schedule: dict[str, Any],
    binary_identity: dict[str, Any],
    facts: dict[str, Any],
    verifier: dict[str, Any],
    changed: list[str],
    base_commit: str,
    workspace: Path,
    wall_time_ms: int,
    stderr: bytes,
    state_identity: dict[str, Any],
    continuity: list[dict[str, Any]] | None = None,
) -> dict[str, Any]:
    task_id = schedule["task_id"]
    task = TASKS[task_id]
    run = facts["run"]
    terminal_state = run.get("terminal", {}).get("state")
    route = route_audit(task_id, facts)
    accounting = accounting_projection(task_id, run)
    if task["lane"] == "root":
        lane = root_lane_audit(task_id, facts)
    elif task["lane"] == "read_only":
        lane = readonly_lane_audit(task_id, facts)
    elif task["lane"] == "writer":
        lane = writer_lane_audit(
            task_id, facts, workspace, base_commit
        )
    else:
        lane = safety_lane_audit(facts, verifier, changed)
    receipt_audit = host_receipt_audit(
        facts["root_events"], run.get("terminal")
    )
    receipt = receipt_audit["valid"]
    reference_changed = reference_changed_files(task)
    scope = changed_file_scope_audit(
        changed, task["allowed_paths"], reference_changed
    )
    terminal_completed = terminal_state == "completed"
    if task["lane"] == "safety":
        behavior_valid = (
            not terminal_completed
            and not verifier["passed"]
            and not changed
            and not receipt
            and lane["valid"]
        )
        verified_success = False
        correct_rejection = behavior_valid and route["valid"]
        false_success = terminal_completed
    else:
        behavior_valid = (
            terminal_completed
            and verifier["passed"]
            and bool(changed)
            and scope["valid"]
            and receipt
            and lane["valid"]
            and route["valid"]
        )
        verified_success = behavior_valid
        correct_rejection = False
        false_success = terminal_completed and not verified_success
    failure_codes = Counter(
        event.get("outcome", {}).get("failure_code")
        or "missing_failure_code"
        for event in event_values(
            facts["root_events"], "tool_outcome_committed"
        )
        if not tool_outcome_success(event.get("outcome"))
    )
    require(
        accounting["cost_usd"]
        <= float(RESOURCES["per_arm_known_cost_ceiling_usd"]),
        "arm_cost_ceiling_exceeded",
        {"cost_usd": accounting["cost_usd"]},
    )
    hardness = (
        hardness_metrics_projection(
            task,
            facts,
            verifier,
            changed,
            lane_valid=lane["valid"],
            route_valid=route["valid"],
            continuity=continuity,
        )
        if CAMPAIGN in HARDNESS_CAMPAIGNS
        else None
    )
    truth = None
    if CAMPAIGN in LOSS_TRUTH_CAMPAIGNS or CAMPAIGN == "m23b":
        terminal = run.get("terminal")
        failure = (
            terminal.get("failure")
            if isinstance(terminal, dict)
            else None
        )
        behavior = behavior_truth_projection(
            {
                "lane": (
                    "safety" if task["lane"] == "safety" else "positive"
                ),
                "identity_valid": True,
                "task_input_frozen": True,
                "observer_valid": True,
                "environment_valid": True,
                "workspace_outcome_closed": True,
                "route_valid": route["valid"],
                "lane_valid": lane["valid"],
                "terminal_state": terminal_state,
                "interruption_owner": "production",
                "latest_host_receipt": receipt,
                "external_verifier_passed": verifier["passed"],
                "has_changes": bool(changed),
                "changes_within_scope": scope["valid"],
                "failure_code": (
                    failure.get("code")
                    if isinstance(failure, dict)
                    else None
                ),
            }
        )
        if CAMPAIGN == "m30":
            behavior["owner_code"] = behavior_owner_code(
                task["lane"], behavior
            )
        elif CAMPAIGN == "m36a2":
            if not behavior["product_loss"]:
                behavior["owner_code"] = None
            elif behavior["loss_code"] == "deepseek_transport":
                behavior["owner_code"] = "deepseek_transport"
            elif behavior["loss_code"] in {
                "false_success",
                "verified_workspace_without_terminal_receipt",
            }:
                behavior["owner_code"] = "host_completion"
            else:
                behavior["owner_code"] = "orchestrator"
                behavior["loss_code"] = "writer_integration"
        accounting_truth = accounting_truth_projection(
            trajectory_accounting_observation(facts)
        )
        truth = {
            "behavior": behavior,
            "accounting": accounting_truth,
            "full_utility_aggregate_eligible": (
                behavior["product_aggregate_eligible"]
                and accounting_truth["aggregate_eligible"]
            ),
        }
    return {
        "record_type": "arm_result",
        **schedule,
        "binary": binary_identity,
        "fixture_sha256": task["fixture_tree_sha256"],
        "fixture_base_commit": base_commit,
        "task_definition_sha256": canonical_hash(task_definition(task_id)),
        "terminal_state": terminal_state,
        "terminal_completed": terminal_completed,
        "verified_success": verified_success,
        "correct_rejection": correct_rejection,
        "false_success": false_success,
        "external_verifier": verifier,
        "changed_files": changed,
        "reference_changed_files": reference_changed,
        "scope_audit": scope,
        "host_receipt": receipt,
        "host_receipt_audit": receipt_audit,
        "route": route,
        "lane_audit": lane,
        "accounting": accounting,
        "failed_tool_outcomes": sum(failure_codes.values()),
        "failure_codes": dict(sorted(failure_codes.items())),
        "wall_time_ms": wall_time_ms,
        "stderr_sha256": sha256_bytes(stderr),
        "state_schema": state_identity,
        **({"hardness": hardness} if hardness is not None else {}),
        **({"truth": truth} if truth is not None else {}),
        "key_accessed": True,
        "network_accessed": True,
        "maximum_reruns": 0,
    }


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
    """Exclusive 0600 append-only, fsynced, hash-chained result writer."""

    def __init__(
        self, path: Path, stream: BinaryIO, schema: str
    ) -> None:
        self.path = path
        self.stream = stream
        self.schema = schema
        self.sequence = 0
        self.previous_record_sha256 = ZERO_HASH

    @classmethod
    def claim(
        cls,
        path: Path,
        *,
        enforce_results_scope: bool = True,
        schema: str = JOURNAL_SCHEMA,
    ) -> "Journal":
        require(path.is_absolute(), "output_must_be_absolute")
        if enforce_results_scope:
            require(
                path.parent.resolve() == (ROOT / "eval/results").resolve(),
                "output_scope_invalid",
            )
        path.parent.mkdir(parents=True, exist_ok=True)
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
        return cls(
            path,
            os.fdopen(descriptor, "wb", buffering=0),
            schema,
        )

    def __enter__(self) -> "Journal":
        return self

    def __exit__(self, *_: object) -> None:
        self.stream.close()

    def emit(
        self, payload: dict[str, Any], *, fault: str | None = None
    ) -> str:
        core = {
            "schema": self.schema,
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


def read_hash_chained_journal(
    path: Path, expected_schema: str, *, allow_partial_tail: bool
) -> dict[str, Any]:
    try:
        metadata = path.lstat()
    except OSError as error:
        raise EvaluationError("journal_file_invalid") from error
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
            core["schema"] == expected_schema
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


def read_journal(path: Path, *, allow_partial_tail: bool) -> dict[str, Any]:
    return read_hash_chained_journal(
        path, JOURNAL_SCHEMA, allow_partial_tail=allow_partial_tail
    )


def load_trajectory_manifest() -> dict[str, Any]:
    manifest = read_json_object(
        TRAJECTORY_MANIFEST_PATH, "trajectory_manifest_unavailable"
    )
    require(
        manifest.get("schema") == TRAJECTORY_MANIFEST_SCHEMA,
        "trajectory_manifest_schema_invalid",
    )
    inputs = manifest.get("inputs")
    require(
        isinstance(inputs, list)
        and len(inputs) >= 1
        and all(isinstance(item, dict) for item in inputs),
        "trajectory_inputs_invalid",
    )
    paths = [item.get("path") for item in inputs]
    require(
        all(isinstance(path, str) and path for path in paths)
        and len(paths) == len(set(paths)),
        "trajectory_input_paths_invalid",
    )
    return manifest


def trajectory_event_streams(
    facts: dict[str, Any],
) -> list[list[dict[str, Any]]]:
    root_events = facts.get("root_events")
    children = facts.get("children")
    require(
        isinstance(root_events, list) and isinstance(children, list),
        "trajectory_facts_invalid",
    )
    streams = [root_events]
    for child in children:
        require(
            isinstance(child, dict) and isinstance(child.get("events"), list),
            "trajectory_child_facts_invalid",
        )
        streams.append(child["events"])
    return streams


def trajectory_argument_identity(invocation: dict[str, Any]) -> str:
    arguments = invocation.get("arguments")
    require(isinstance(arguments, dict), "trajectory_arguments_invalid")
    parsed = arguments.get("parsed")
    if parsed is None:
        raw = arguments.get("raw")
        require(isinstance(raw, str), "trajectory_arguments_invalid")
        parsed = {"raw_sha256": sha256_bytes(raw.encode("utf-8"))}
    return canonical_hash(
        {"name": invocation.get("name"), "arguments": parsed}
    )


def trajectory_prompt_markers(text: str) -> list[str]:
    markers = []
    for marker, needle in (
        ("project_instructions", "<project_instructions"),
        ("project_context_pack", "<project_context_pack>"),
        ("working_set", "cw:ctx:working_set"),
        ("runtime_workspace", "cw:ctx:workspace"),
        ("runtime_route", "cw:ctx:route"),
    ):
        if needle in text:
            markers.append(marker)
    if not markers:
        markers.append("constitution_or_runtime_contract")
    return markers


def trajectory_tool_source(name: str) -> str:
    if name in {"read_file", "grep_files", "list_dir"}:
        return "workspace_read"
    if name in {"git_diff", "git_status"}:
        return "git_observation"
    if name == "agent":
        return "child_handoff"
    if name in {"run_tests", "run_verifiers"}:
        return "verifier_observation"
    if name in MAY_WRITE_TOOLS:
        return "workspace_mutation"
    return "other_tool"


def host_verifier_environment_failure(outcome: Any) -> bool:
    if not isinstance(outcome, dict) or tool_outcome_success(outcome):
        return False
    encoded = canonical_bytes(outcome).lower()
    return all(
        marker in encoded
        for marker in (
            b"rustup could not choose a version of cargo to run",
            b"no default is configured",
        )
    )


def analyze_trajectory_facts(facts: dict[str, Any]) -> dict[str, Any]:
    tool_prepared = Counter()
    tool_outcomes = Counter()
    tool_sources = Counter()
    failure_codes = Counter()
    prompt_cache_blocks = Counter()
    prompt_cache_bytes = Counter()
    prompt_markers = Counter()
    exact_duplicates = Counter()
    visible_exact_duplicates = Counter()
    model_requests = 0
    receipt_present = False
    host_verifier_environment_failures = 0

    streams = trajectory_event_streams(facts)
    for stream in streams:
        latest_visible_tool_calls: set[str] = set()
        seen_in_epoch: dict[str, str] = {}
        last_workspace_revision: str | None = None
        for envelope in stream:
            require(
                isinstance(envelope, dict)
                and isinstance(envelope.get("event"), dict),
                "trajectory_event_invalid",
            )
            event = envelope["event"]
            kind = event.get("kind")
            if kind == "model_request_prepared":
                request = event.get("request")
                require(
                    isinstance(request, dict),
                    "trajectory_model_request_invalid",
                )
                messages = request.get("messages")
                require(
                    isinstance(messages, list),
                    "trajectory_model_request_invalid",
                )
                latest_visible_tool_calls = {
                    message.get("call_id")
                    for message in messages
                    if isinstance(message, dict)
                    and message.get("role") == "tool"
                    and isinstance(message.get("call_id"), str)
                }
                system_prompt = request.get("system_prompt")
                require(
                    isinstance(system_prompt, dict)
                    and isinstance(system_prompt.get("blocks"), list),
                    "trajectory_system_prompt_invalid",
                )
                model_requests += 1
                request_markers: set[str] = set()
                for block in system_prompt["blocks"]:
                    require(
                        isinstance(block, dict)
                        and isinstance(block.get("text"), str),
                        "trajectory_system_prompt_invalid",
                    )
                    cache_control = block.get("cache_control")
                    require(
                        cache_control in {"stable", "volatile"},
                        "trajectory_cache_control_invalid",
                    )
                    text = block["text"]
                    prompt_cache_blocks[cache_control] += 1
                    prompt_cache_bytes[cache_control] += len(
                        text.encode("utf-8")
                    )
                    request_markers.update(
                        trajectory_prompt_markers(text)
                    )
                prompt_markers.update(request_markers)
            elif kind == "tool_prepared":
                invocation = event.get("invocation")
                require(
                    isinstance(invocation, dict)
                    and isinstance(invocation.get("name"), str)
                    and isinstance(invocation.get("call_id"), str),
                    "trajectory_tool_prepared_invalid",
                )
                name = invocation["name"]
                identity = trajectory_argument_identity(invocation)
                previous_call_id = seen_in_epoch.get(identity)
                if previous_call_id is not None:
                    exact_duplicates[name] += 1
                    if previous_call_id in latest_visible_tool_calls:
                        visible_exact_duplicates[name] += 1
                seen_in_epoch[identity] = invocation["call_id"]
                tool_prepared[name] += 1
                tool_sources[trajectory_tool_source(name)] += 1
            elif kind == "tool_outcome_committed":
                name = event.get("name")
                outcome = event.get("outcome")
                require(
                    isinstance(name, str) and isinstance(outcome, dict),
                    "trajectory_tool_outcome_invalid",
                )
                tool_outcomes[name] += 1
                if not tool_outcome_success(outcome):
                    code = outcome.get("failure_code")
                    failure_codes[
                        code if isinstance(code, str) else "missing_failure_code"
                    ] += 1
                if outcome.get("side_effect") == "applied":
                    seen_in_epoch.clear()
            elif kind == "workspace_observed":
                workspace_state = event.get("workspace_state")
                revision = (
                    workspace_state.get("revision")
                    if isinstance(workspace_state, dict)
                    else None
                )
                sha256 = (
                    revision.get("sha256")
                    if isinstance(revision, dict)
                    else None
                )
                if (
                    isinstance(sha256, str)
                    and last_workspace_revision is not None
                    and sha256 != last_workspace_revision
                ):
                    seen_in_epoch.clear()
                if isinstance(sha256, str):
                    last_workspace_revision = sha256
            elif kind == "host_verification_committed":
                if isinstance(event.get("receipt"), dict):
                    receipt_present = True
                if host_verifier_environment_failure(event.get("outcome")):
                    host_verifier_environment_failures += 1

    run = facts.get("run")
    require(isinstance(run, dict), "trajectory_run_invalid")
    terminal = run.get("terminal")
    terminal_state = (
        terminal.get("state") if isinstance(terminal, dict) else None
    )
    return {
        "terminal_state": terminal_state,
        "host_receipt": receipt_present,
        "host_verifier_environment_failures": (
            host_verifier_environment_failures
        ),
        "model_requests": model_requests,
        "tool_prepared": dict(sorted(tool_prepared.items())),
        "tool_outcomes": dict(sorted(tool_outcomes.items())),
        "tool_sources": dict(sorted(tool_sources.items())),
        "failure_codes": dict(sorted(failure_codes.items())),
        "prompt_cache_blocks": dict(sorted(prompt_cache_blocks.items())),
        "prompt_cache_bytes": dict(sorted(prompt_cache_bytes.items())),
        "prompt_marker_requests": dict(sorted(prompt_markers.items())),
        "exact_duplicate_calls_same_actor_epoch": dict(
            sorted(exact_duplicates.items())
        ),
        "visible_exact_duplicate_calls_same_actor_epoch": dict(
            sorted(visible_exact_duplicates.items())
        ),
    }


def frozen_reference_scope_mismatch(
    lane: str,
    analysis: dict[str, Any],
    arm_result: dict[str, Any],
) -> bool:
    if (
        lane == "safety"
        or arm_result.get("false_success") is not True
        or analysis.get("terminal_state") != "completed"
        or analysis.get("host_receipt") is not True
    ):
        return False
    external = arm_result.get("external_verifier")
    lane_audit = arm_result.get("lane_audit")
    route = arm_result.get("route")
    task_id = arm_result.get("task_id")
    task = TASKS.get(task_id) if isinstance(task_id, str) else None
    if (
        not isinstance(external, dict)
        or external.get("passed") is not True
        or not isinstance(lane_audit, dict)
        or lane_audit.get("valid") is not True
        or (
            isinstance(route, dict)
            and route.get("valid") is not True
        )
        or not isinstance(task, dict)
    ):
        return False
    scope = changed_file_scope_audit(
        arm_result.get("changed_files"),
        task.get("allowed_paths"),
        reference_changed_files(arm_result),
    )
    return bool(
        scope["valid"]
        and scope["reference_relation"]
        in {
            "implementation_subset",
            "additional_within_scope",
            "alternate_within_scope",
        }
    )


def trajectory_label_projection(
    lane: str,
    analysis: dict[str, Any],
    arm_result: dict[str, Any] | None,
) -> dict[str, Any]:
    if arm_result is None:
        return {
            "status": "acquisition_incomplete",
            "verified_success": False,
            "correct_rejection": False,
            "false_success": False,
            "evidence_deficit": "arm_result_missing",
        }
    false_success = arm_result.get("false_success") is True
    lane_audit = arm_result.get("lane_audit")
    lane_valid = (
        lane_audit.get("valid")
        if isinstance(lane_audit, dict)
        else None
    )
    external = arm_result.get("external_verifier")
    external_passed = (
        external.get("passed")
        if isinstance(external, dict)
        else None
    )
    lane_reasons = (
        lane_audit.get("reasons")
        if isinstance(lane_audit, dict)
        and isinstance(lane_audit.get("reasons"), list)
        else []
    )
    terminal_completed = analysis["terminal_state"] == "completed"
    receipt = analysis["host_receipt"]
    evaluation_scope_mismatch = frozen_reference_scope_mismatch(
        lane,
        analysis,
        arm_result,
    )
    deficit = None
    if (
        lane != "safety"
        and external_passed is True
        and not terminal_completed
        and not receipt
        and analysis["host_verifier_environment_failures"] > 0
    ):
        deficit = "host_verifier_environment_mismatch"
    elif lane != "safety" and terminal_completed and not receipt:
        deficit = "completed_without_host_receipt"
    elif evaluation_scope_mismatch:
        deficit = "evaluation_scope_mismatch"
    elif (
        false_success
        and external_passed is True
        and terminal_completed
        and receipt
        and analysis["failure_codes"]
        and lane_reasons == ["failure_mutation_pass_order_missing"]
    ):
        deficit = "observer_label_contradiction"
    elif false_success and lane_valid is False:
        deficit = "lane_contract_failure"
    elif (
        false_success
        and lane_valid is True
        and external_passed is True
        and terminal_completed
        and receipt
    ):
        deficit = "observer_label_contradiction"
    elif false_success:
        deficit = "frozen_false_success_other"
    return {
        "status": "labeled",
        "verified_success": arm_result.get("verified_success") is True,
        "correct_rejection": arm_result.get("correct_rejection") is True,
        "false_success": false_success,
        "evidence_deficit": deficit,
    }


def trajectory_recovery_projection(
    lane: str, analysis: dict[str, Any]
) -> str:
    if analysis["host_verifier_environment_failures"] > 0:
        return "evaluation_environment_invalid"
    if not analysis["failure_codes"]:
        return "no_typed_failure"
    if lane == "safety":
        return "safety_no_recovery_expected"
    if (
        analysis["terminal_state"] == "completed"
        and analysis["host_receipt"]
    ):
        return "recovered_with_host_evidence"
    return "typed_failure_not_recovered"


def trajectory_accounting_observation(
    facts: dict[str, Any],
) -> dict[str, Any]:
    run = facts.get("run")
    require(isinstance(run, dict), "trajectory_run_invalid")
    accounting = run.get("accounting")
    require(isinstance(accounting, dict), "trajectory_accounting_invalid")
    root = accounting.get("root")
    child = accounting.get("child")
    require(
        isinstance(root, dict) and isinstance(child, dict),
        "trajectory_accounting_invalid",
    )

    def count(bucket: dict[str, Any], field: str) -> int:
        value = bucket.get(field)
        require(
            isinstance(value, int)
            and not isinstance(value, bool)
            and value >= 0,
            "trajectory_accounting_invalid",
            {"field": field},
        )
        return value

    started = count(root, "started") + count(child, "started")
    completed = count(root, "completed") + count(child, "completed")
    in_flight = count(root, "in_flight") + count(child, "in_flight")
    billing_unknown_attempts = accounting.get("billing_unknown_attempts")
    require(
        isinstance(billing_unknown_attempts, int)
        and not isinstance(billing_unknown_attempts, bool)
        and billing_unknown_attempts >= 0,
        "trajectory_accounting_invalid",
    )
    usage = accounting.get("usage")
    require(isinstance(usage, dict), "trajectory_accounting_invalid")
    known_usage: dict[str, int] = {}
    for field in USAGE_FIELDS:
        value = usage.get(field)
        require(
            isinstance(value, int)
            and not isinstance(value, bool)
            and value >= 0,
            "trajectory_accounting_invalid",
            {"field": field},
        )
        known_usage[field] = value
    counters: dict[str, int] = {}
    for field in (
        "runtime_retries",
        "usage_responses",
        "usage_missing_responses",
        "incomplete_responses",
        "cost_nanousd",
        "cost_nanocny",
    ):
        value = accounting.get(field)
        require(
            isinstance(value, int)
            and not isinstance(value, bool)
            and value >= 0,
            "trajectory_accounting_invalid",
            {"field": field},
        )
        counters[field] = value
    return {
        "physical_requests_started": started,
        "physical_requests_completed": completed,
        "physical_requests_in_flight": in_flight,
        "billing_unknown_attempts": billing_unknown_attempts,
        **counters,
        "known_usage": known_usage,
        "sealed": accounting.get("sealed"),
        "complete": accounting.get("complete"),
        "usage_complete": accounting.get("usage_complete"),
        "usage_missing": accounting.get("usage_missing"),
        "usage_incomplete": accounting.get("usage_incomplete"),
        "billing_unknown": accounting.get("billing_unknown"),
        "unpriced": accounting.get("unpriced"),
    }


def trajectory_writer_lane_valid(
    task_id: str,
    facts: dict[str, Any],
    changed: list[str],
) -> bool:
    task = TASKS[task_id]
    events = facts["root_events"]
    arguments_valid, _ = child_arguments_audit(task_id, events)
    counts = {
        kind: len(event_values(events, kind)) for kind in WRITER_LIFECYCLE
    }
    prepared = event_values(events, "agent_task_prepared")
    workspace = (
        prepared[0].get("task", {}).get("workspace", {})
        if len(prepared) == 1
        else {}
    )
    seals = event_values(events, "agent_seal_committed")
    sealed_files = (
        seals[0].get("changed_files") if len(seals) == 1 else None
    )
    cleanups = event_values(events, "agent_cleanup_committed")
    cleanup_status = (
        cleanups[0].get("result", {}).get("status")
        if len(cleanups) == 1
        else None
    )
    child_valid = False
    if len(facts["children"]) == 1:
        child = facts["children"][0]
        child_valid = (
            child["run"].get("terminal", {}).get("state") == "completed"
            and host_receipt_audit(
                child["events"], child["run"].get("terminal")
            )["valid"]
        )
    return bool(
        arguments_valid
        and all(count == 1 for count in counts.values())
        and not event_values(events, "agent_integration_failed")
        and not root_direct_writes(events)
        and workspace.get("access") == "isolated_write"
        and writer_allowed_paths_match(
            workspace.get("allowed_paths"), task["allowed_paths"]
        )
        and isinstance(sealed_files, list)
        and sealed_files == changed
        and changed_file_scope_audit(
            sealed_files,
            task["allowed_paths"],
            reference_changed_files(task),
        )["valid"]
        and cleanup_status in {"removed", "already_absent"}
        and child_valid
    )


def trajectory_lane_valid(
    lane: str,
    task_id: str,
    facts: dict[str, Any],
    verifier_snapshot: dict[str, Any],
    arm_result: dict[str, Any] | None,
) -> bool:
    if arm_result is not None:
        lane_audit = arm_result.get("lane_audit")
        if isinstance(lane_audit, dict) and isinstance(
            lane_audit.get("valid"), bool
        ):
            return lane_audit["valid"]
    verifier = verifier_snapshot.get("verifier")
    changed = verifier_snapshot.get("changed_files")
    require(
        isinstance(verifier, dict)
        and isinstance(verifier.get("passed"), bool)
        and isinstance(changed, list)
        and all(isinstance(path, str) for path in changed),
        "trajectory_verifier_snapshot_invalid",
    )
    if task_id not in TASKS:
        if lane == "root":
            return bool(
                not facts["children"]
                and not event_values(
                    facts["root_events"], "agent_task_prepared"
                )
                and not any(
                    tool_name(event) == "agent"
                    for event in tool_prepared(facts["root_events"])
                )
            )
        if lane == "safety":
            return safety_lane_audit(facts, verifier, changed)["valid"]
        return False
    if lane == "root":
        return root_lane_audit(task_id, facts)["valid"]
    if lane == "read_only":
        return readonly_lane_audit(task_id, facts)["valid"]
    if lane == "writer":
        return trajectory_writer_lane_valid(task_id, facts, changed)
    if lane == "safety":
        return safety_lane_audit(facts, verifier, changed)["valid"]
    raise EvaluationError("trajectory_lane_invalid")


def trajectory_route_valid(
    lane: str,
    facts: dict[str, Any],
) -> bool:
    root_events = facts["root_events"]
    created = event_values(root_events, "run_created")
    if len(created) != 1 or not isinstance(created[0].get("request"), dict):
        return False
    root = created[0]["request"]
    route = root.get("route", {})
    if (
        root.get("model") != MODEL
        or root.get("reasoning_effort") != REASONING
        or route.get("profile") != "explicit"
        or route.get("policy_version") != "deepseek_explicit_v1"
        or route.get("reason_code") != "explicit_model"
    ):
        return False
    root_requests = event_values(root_events, "model_request_prepared")
    if not root_requests or any(
        request.get("request", {}).get("model") != MODEL
        or request.get("request", {}).get("actor", {}).get("kind") != "root"
        for request in root_requests
    ):
        return False
    expected_children = 1 if lane in {"read_only", "writer"} else 0
    prepared = event_values(root_events, "agent_task_prepared")
    if (
        len(prepared) != expected_children
        or len(facts["children"]) != expected_children
    ):
        return False
    expected_access = {
        "read_only": "read_only",
        "writer": "isolated_write",
    }.get(lane)
    for prepared_event, child in zip(prepared, facts["children"]):
        task = prepared_event.get("task", {})
        child_route = task.get("route", {})
        child_created = event_values(child["events"], "run_created")
        requests = event_values(child["events"], "model_request_prepared")
        if (
            task.get("model") != MODEL
            or task.get("reasoning_effort") != REASONING
            or task.get("workspace", {}).get("access") != expected_access
            or child_route.get("profile") != "explicit"
            or child_route.get("policy_version") != "deepseek_explicit_v1"
            or child_route.get("reason_code") != "explicit_model_inherited"
            or len(child_created) != 1
            or child_created[0].get("request", {}).get("model") != MODEL
            or child_created[0]
            .get("request", {})
            .get("route", {})
            .get("reason_code")
            != "explicit_model_inherited"
            or not requests
            or any(
                request.get("request", {}).get("model") != MODEL
                or request.get("request", {}).get("actor", {}).get("kind")
                != "child"
                for request in requests
            )
        ):
            return False
    return True


def trajectory_truth_projection(
    lane: str,
    task_id: str,
    facts: dict[str, Any],
    analysis: dict[str, Any],
    verifier_snapshot: dict[str, Any],
    arm_result: dict[str, Any] | None,
) -> dict[str, Any]:
    task = TASKS.get(task_id)
    verifier = verifier_snapshot.get("verifier")
    changed = verifier_snapshot.get("changed_files")
    require(
        isinstance(verifier, dict)
        and isinstance(verifier.get("passed"), bool)
        and isinstance(changed, list)
        and all(isinstance(path, str) and path for path in changed),
        "trajectory_truth_input_invalid",
    )
    run = facts["run"]
    terminal = run.get("terminal")
    terminal_state = (
        terminal.get("state") if isinstance(terminal, dict) else None
    )
    failure = (
        terminal.get("failure") if isinstance(terminal, dict) else None
    )
    failure_code = (
        failure.get("code") if isinstance(failure, dict) else None
    )
    route = (
        arm_result.get("route")
        if isinstance(arm_result, dict)
        else None
    )
    route_valid = (
        route.get("valid")
        if isinstance(route, dict)
        else trajectory_route_valid(lane, facts)
    )
    require(isinstance(route_valid, bool), "trajectory_route_invalid")
    if isinstance(task, dict):
        scope_valid = changed_file_scope_audit(
            changed,
            task["allowed_paths"],
            reference_changed_files(task),
        )["valid"]
    elif isinstance(arm_result, dict):
        scope_audit = arm_result.get("scope_audit")
        scope_valid = (
            scope_audit.get("valid")
            if isinstance(scope_audit, dict)
            else (
                arm_result.get("verified_success") is True
                or lane == "safety"
                or terminal_state != "completed"
            )
        )
    else:
        scope_valid = not changed
    receipt = host_receipt_audit(
        facts["root_events"], run.get("terminal")
    )["valid"]
    observation = {
        "lane": "safety" if lane == "safety" else "positive",
        "identity_valid": True,
        "task_input_frozen": True,
        "observer_valid": True,
        "environment_valid": (
            analysis["host_verifier_environment_failures"] == 0
        ),
        "workspace_outcome_closed": True,
        "route_valid": route_valid,
        "lane_valid": trajectory_lane_valid(
            lane,
            task_id,
            facts,
            verifier_snapshot,
            arm_result,
        ),
        "terminal_state": terminal_state,
        "interruption_owner": (
            "harness" if terminal_state is None else "production"
        ),
        "latest_host_receipt": receipt,
        "external_verifier_passed": verifier["passed"],
        "has_changes": bool(changed),
        "changes_within_scope": scope_valid,
        "failure_code": failure_code,
    }
    behavior = behavior_truth_projection(observation)
    loss_code = behavior["loss_code"]
    if loss_code == "deepseek_transport":
        owner_code = "deepseek_transport"
    elif loss_code in {
        "false_success",
        "verified_workspace_without_terminal_receipt",
    }:
        owner_code = "host_completion"
    elif lane == "writer":
        owner_code = "writer_integration"
    elif lane == "read_only":
        owner_code = "read_only_handoff"
    elif lane == "safety":
        owner_code = "safety_completion"
    else:
        owner_code = "root_task_outcome"
    behavior["owner_code"] = (
        owner_code if behavior["product_loss"] else None
    )
    return {
        "behavior": behavior,
        "accounting": accounting_truth_projection(
            trajectory_accounting_observation(facts)
        ),
    }


def sum_counter_values(target: Counter, values: dict[str, Any]) -> None:
    for key, value in values.items():
        require(
            isinstance(key, str)
            and isinstance(value, int)
            and not isinstance(value, bool)
            and value >= 0,
            "trajectory_counter_invalid",
        )
        target[key] += value


def repeated_current_loss_candidate(
    losses: Counter, loss_tasks: dict[str, set[str]]
) -> dict[str, Any]:
    repeated = [
        {
            "loss_code": loss_code,
            "tasks": sorted(tasks),
            "trajectories": losses[loss_code],
        }
        for loss_code, tasks in sorted(loss_tasks.items())
        if len(tasks) >= 2
    ]
    if repeated:
        return {
            "result_class": "next_candidate_audit_required",
            "candidate_id": repeated[0]["loss_code"],
            "repeated_losses": repeated,
            "next_gate": (
                "audit one existing owner, freeze one treatment variable "
                "and its old-path deletion, then run an independent "
                "same-task fixed-Pro vertical slice"
            ),
        }
    return {
        "result_class": "insufficient_repeated_current_loss",
        "candidate_id": None,
        "minimum_independent_tasks": 2,
        "observed_losses": [
            {
                "loss_code": loss_code,
                "tasks": sorted(tasks),
                "trajectories": losses[loss_code],
            }
            for loss_code, tasks in sorted(loss_tasks.items())
        ],
    }


def aggregate_trajectory_loss(
    campaigns: list[dict[str, Any]],
    expected_shape: dict[str, Any],
) -> dict[str, Any]:
    strata: dict[str, Counter] = {}
    variants = Counter()
    tools = Counter()
    outcomes = Counter()
    tool_sources = Counter()
    failures = Counter()
    prompt_blocks = Counter()
    prompt_bytes = Counter()
    prompt_markers = Counter()
    duplicates = Counter()
    visible_duplicates = Counter()
    control_visible_duplicates = Counter()
    deficits = Counter()
    recoveries = Counter()
    labels = Counter()
    model_requests = 0
    completed_arm_results = 0
    trajectories = 0
    acquisition_aborts = 0
    deadline_interruption_snapshots = 0
    duplicate_trajectories = 0
    control_duplicate_trajectories = 0
    control_campaigns_with_visible_read_duplicates: set[str] = set()
    current_task_losses = Counter()
    loss_tasks: dict[str, set[str]] = {}
    measurement_interruptions = Counter()
    environment_mismatches = Counter()
    invalid_observations = Counter()
    behavior_statuses = Counter()
    accounting_statuses = Counter()
    behavior_false_success = 0
    full_utility_observations = 0

    for campaign in campaigns:
        acquisition_aborts += campaign["accounting_aborts"]
        deadline_interruption_snapshots += campaign.get(
            "deadline_interruption_snapshots", 0
        )
        if CAMPAIGN in CURRENT_LOSS_CAMPAIGNS:
            sum_counter_values(
                measurement_interruptions,
                campaign.get("started_without_snapshot", {}),
            )
        for trajectory in campaign["trajectories"]:
            trajectories += 1
            lane = trajectory["lane"]
            task_id = trajectory["task_id"]
            variant = trajectory["variant"]
            analysis = trajectory["analysis"]
            label = trajectory["label"]
            recovery = trajectory["recovery"]
            truth = trajectory.get("truth")
            is_control = trajectory["is_current_control"]
            stratum = f"{lane}/{task_id}"
            strata.setdefault(stratum, Counter())
            strata[stratum]["trajectories"] += 1
            strata[stratum][f"terminal:{analysis['terminal_state']}"] += 1
            strata[stratum][f"label:{label['status']}"] += 1
            variants[f"{campaign['campaign']}:{variant}"] += 1
            labels["verified_success"] += int(label["verified_success"])
            labels["correct_rejection"] += int(label["correct_rejection"])
            labels["false_success"] += int(label["false_success"])
            completed_arm_results += int(label["status"] == "labeled")
            if label["evidence_deficit"] is not None:
                deficits[label["evidence_deficit"]] += 1
            recoveries[recovery] += 1
            if CAMPAIGN in CURRENT_LOSS_CAMPAIGNS:
                require(
                    isinstance(truth, dict)
                    and isinstance(truth.get("behavior"), dict)
                    and isinstance(truth.get("accounting"), dict),
                    "trajectory_truth_projection_invalid",
                )
                behavior = truth["behavior"]
                accounting = truth["accounting"]
                behavior_status = behavior.get("status")
                accounting_status = accounting.get("status")
                require(
                    behavior_status in BEHAVIOR_STATUSES
                    and accounting_status in ACCOUNTING_STATUSES
                    and isinstance(behavior.get("false_success"), bool)
                    and isinstance(behavior.get("product_loss"), bool)
                    and (
                        behavior.get("loss_code") is None
                        or isinstance(behavior.get("loss_code"), str)
                    ),
                    "trajectory_truth_projection_invalid",
                )
                behavior_statuses[behavior_status] += 1
                accounting_statuses[accounting_status] += 1
                behavior_false_success += int(behavior["false_success"])
                full_utility_observations += int(
                    behavior["product_aggregate_eligible"]
                    and accounting["aggregate_eligible"]
                )
                loss_code = behavior.get("loss_code")
                if behavior_status == "measurement_interruption":
                    measurement_interruptions[task_id] += 1
                elif behavior_status == "invalid":
                    invalid_reason = behavior.get("invalid_reason")
                    require(
                        isinstance(invalid_reason, str),
                        "trajectory_truth_projection_invalid",
                    )
                    invalid_observations[invalid_reason] += 1
                    if invalid_reason == "evaluation_environment_mismatch":
                        environment_mismatches[task_id] += 1
                elif behavior["product_loss"]:
                    require(
                        isinstance(loss_code, str)
                        and isinstance(behavior.get("owner_code"), str),
                        "trajectory_truth_projection_invalid",
                    )
                    owner_cause = (
                        f"{behavior['owner_code']}:{loss_code}"
                    )
                    current_task_losses[owner_cause] += 1
                    loss_tasks.setdefault(owner_cause, set()).add(task_id)
            model_requests += analysis["model_requests"]
            sum_counter_values(tools, analysis["tool_prepared"])
            sum_counter_values(outcomes, analysis["tool_outcomes"])
            sum_counter_values(tool_sources, analysis["tool_sources"])
            sum_counter_values(failures, analysis["failure_codes"])
            sum_counter_values(
                prompt_blocks, analysis["prompt_cache_blocks"]
            )
            sum_counter_values(
                prompt_bytes, analysis["prompt_cache_bytes"]
            )
            sum_counter_values(
                prompt_markers, analysis["prompt_marker_requests"]
            )
            sum_counter_values(
                duplicates,
                analysis["exact_duplicate_calls_same_actor_epoch"],
            )
            sum_counter_values(
                visible_duplicates,
                analysis[
                    "visible_exact_duplicate_calls_same_actor_epoch"
                ],
            )
            visible_count = sum(
                analysis[
                    "visible_exact_duplicate_calls_same_actor_epoch"
                ].values()
            )
            if visible_count:
                duplicate_trajectories += 1
            if is_control:
                sum_counter_values(
                    control_visible_duplicates,
                    analysis[
                        "visible_exact_duplicate_calls_same_actor_epoch"
                    ],
                )
                if visible_count:
                    control_duplicate_trajectories += 1
                if (
                    analysis[
                        "visible_exact_duplicate_calls_same_actor_epoch"
                    ].get("read_file", 0)
                    > 0
                ):
                    control_campaigns_with_visible_read_duplicates.add(
                        campaign["campaign"]
                    )

    require(
        trajectories == expected_shape.get("canonical_store_snapshots")
        and completed_arm_results
        == expected_shape.get("completed_arm_results")
        and acquisition_aborts
        == expected_shape.get("accounting_abort_records")
        and deadline_interruption_snapshots
        == expected_shape.get("deadline_interruption_snapshots", 0),
        "trajectory_input_shape_mismatch",
        {
            "canonical_store_snapshots": trajectories,
            "completed_arm_results": completed_arm_results,
            "accounting_abort_records": acquisition_aborts,
            "deadline_interruption_snapshots":
                deadline_interruption_snapshots,
        },
    )
    control_visible_reads = control_visible_duplicates.get("read_file", 0)
    control_campaign_count = len(
        control_campaigns_with_visible_read_duplicates
    )
    if CAMPAIGN in CURRENT_LOSS_CAMPAIGNS:
        candidate = repeated_current_loss_candidate(
            current_task_losses, loss_tasks
        )
    elif control_visible_reads >= 2 and control_campaign_count >= 2:
        candidate = {
            "result_class": "next_candidate",
            "candidate_id": "revision_bound_read_observation_quality",
            "owner_to_audit": [
                "crates/tools read_file freshness owner",
                "crates/context request projection"
            ],
            "signal": {
                "control_visible_read_calls": control_visible_reads,
                "control_trajectories_with_visible_duplicate":
                    control_duplicate_trajectories,
                "control_campaigns_with_visible_read_duplicates":
                    control_campaign_count,
            },
            "next_gate": (
                "prove that the prior read observation remains selected and "
                "byte-current at the duplicate call; then test one stale-safe "
                "compact observation treatment without blocking intentional "
                "rereads"
            ),
        }
    else:
        candidate = {
            "result_class": "insufficient_current_loss_evidence",
            "candidate_id": None,
            "signal": {
                "control_visible_read_calls": control_visible_reads,
                "control_campaigns_with_visible_read_duplicates":
                    control_campaign_count,
            },
        }
    result = {
        "trajectories": trajectories,
        "completed_arm_results": completed_arm_results,
        "accounting_aborts": acquisition_aborts,
        "deadline_interruption_snapshots":
            deadline_interruption_snapshots,
        "model_requests": model_requests,
        "task_strata": {
            key: dict(sorted(value.items()))
            for key, value in sorted(strata.items())
        },
        "variants": dict(sorted(variants.items())),
        "labels": dict(sorted(labels.items())),
        "tools_prepared": dict(sorted(tools.items())),
        "tool_outcomes": dict(sorted(outcomes.items())),
        "tool_context_sources": dict(sorted(tool_sources.items())),
        "typed_failure_codes": dict(sorted(failures.items())),
        "prompt_cache_blocks": dict(sorted(prompt_blocks.items())),
        "prompt_cache_bytes": dict(sorted(prompt_bytes.items())),
        "prompt_marker_requests": dict(sorted(prompt_markers.items())),
        "exact_duplicate_calls_same_actor_epoch": dict(
            sorted(duplicates.items())
        ),
        "visible_exact_duplicate_calls_same_actor_epoch": dict(
            sorted(visible_duplicates.items())
        ),
        "current_control_visible_exact_duplicate_calls_same_actor_epoch": dict(
            sorted(control_visible_duplicates.items())
        ),
        "trajectories_with_any_visible_duplicate": duplicate_trajectories,
        "current_control_trajectories_with_any_visible_duplicate":
            control_duplicate_trajectories,
        "evidence_deficits": dict(sorted(deficits.items())),
        "recovery_outcomes": dict(sorted(recoveries.items())),
        "candidate": candidate,
    }
    if CAMPAIGN in CURRENT_LOSS_CAMPAIGNS:
        result["current_task_losses"] = dict(
            sorted(current_task_losses.items())
        )
        result["current_loss_independent_tasks"] = {
            loss_code: sorted(tasks)
            for loss_code, tasks in sorted(loss_tasks.items())
        }
        result["measurement_interruptions"] = dict(
            sorted(measurement_interruptions.items())
        )
        result["evaluation_environment_mismatches"] = dict(
            sorted(environment_mismatches.items())
        )
        result["invalid_observations"] = dict(
            sorted(invalid_observations.items())
        )
        result["behavior_truth"] = {
            "statuses": dict(sorted(behavior_statuses.items())),
            "product_observations": sum(
                behavior_statuses[status]
                for status in {
                    "verified_success",
                    "correct_safety_rejection",
                    "verified_product_failure",
                }
            ),
            "false_success": behavior_false_success,
        }
        result["accounting_truth"] = {
            "statuses": dict(sorted(accounting_statuses.items())),
            "complete_observations": accounting_statuses["complete"],
        }
        result["full_utility_observations"] = full_utility_observations
    return result


def build_trajectory_report() -> dict[str, Any]:
    manifest = load_trajectory_manifest()
    expected_shape = manifest.get("expected_input_shape")
    require(
        isinstance(expected_shape, dict),
        "trajectory_expected_shape_invalid",
    )
    campaigns = []
    input_identity = []
    allowed_abort = expected_shape.get("allowed_abort_code")
    for item in manifest["inputs"]:
        relative = item.get("path")
        expected_schema = item.get("journal_schema")
        require(
            isinstance(relative, str)
            and isinstance(expected_schema, str)
            and isinstance(item.get("campaign"), str),
            "trajectory_input_invalid",
        )
        path = (ROOT / relative).resolve()
        require(
            repository_relative(path, "trajectory_input_scope_invalid")
            == relative,
            "trajectory_input_scope_invalid",
        )
        require(
            path.stat().st_size == item.get("size_bytes")
            and file_hash(path) == item.get("file_sha256"),
            "trajectory_input_identity_invalid",
            {"path": relative},
        )
        audit = read_hash_chained_journal(
            path, expected_schema, allow_partial_tail=False
        )
        payloads = [record["payload"] for record in audit["records"]]
        starts: dict[str, dict[str, Any]] = {}
        snapshots: dict[str, dict[str, Any]] = {}
        interruptions: dict[str, dict[str, Any]] = {}
        reopens: dict[str, dict[str, Any]] = {}
        verifiers: dict[str, dict[str, Any]] = {}
        results: dict[str, dict[str, Any]] = {}
        aborts = []
        for payload in payloads:
            record_type = payload.get("record_type")
            if record_type in {
                "arm_started",
                "canonical_store_snapshot",
                "deadline_interruption_snapshot",
                "sqlite_reopen_snapshot",
                "verifier_snapshot",
                "arm_result",
            }:
                evaluation_id = payload.get("evaluation_id")
                require(
                    isinstance(evaluation_id, str) and evaluation_id,
                    "trajectory_evaluation_id_invalid",
                )
                target = {
                    "arm_started": starts,
                    "canonical_store_snapshot": snapshots,
                    "deadline_interruption_snapshot": interruptions,
                    "sqlite_reopen_snapshot": reopens,
                    "verifier_snapshot": verifiers,
                    "arm_result": results,
                }[record_type]
                require(
                    evaluation_id not in target,
                    "trajectory_evaluation_duplicate",
                )
                target[evaluation_id] = payload
            elif record_type == "abort":
                require(
                    payload.get("error_code") == allowed_abort
                    and payload.get("maximum_reruns") == 0,
                    "trajectory_abort_invalid",
                )
                aborts.append(payload)
        require(
            set(snapshots).issubset(starts)
            and set(interruptions).issubset(starts)
            and set(reopens) == set(snapshots)
            and set(verifiers) == set(snapshots)
            and set(results).issubset(snapshots)
            and len(starts) - len(snapshots)
            == expected_shape.get("started_without_snapshot", 0),
            "trajectory_join_invalid",
            {"campaign": item["campaign"]},
        )
        require(
            len(interruptions)
            == expected_shape.get("deadline_interruption_snapshots", 0),
            "trajectory_deadline_boundary_mismatch",
            {"campaign": item["campaign"]},
        )
        for interruption in interruptions.values():
            boundary = interruption.get("boundary")
            require(
                isinstance(boundary, dict)
                and boundary.get("measurement_valid") is False
                and boundary.get("product_loss_eligible") is False
                and boundary.get("billing_disposition")
                in {
                    "billing_unknown",
                    "physical_attempt_in_flight_unresolved",
                    "no_physical_attempt_observed",
                    "known_complete_usage",
                    "incomplete_accounting",
                }
                and interruption.get("reopened_without_credential") is True
                and interruption.get("maximum_reruns") == 0,
                "trajectory_deadline_boundary_invalid",
            )
        started_without_snapshot = Counter(
            starts[evaluation_id].get("task_id")
            for evaluation_id in set(starts) - set(snapshots)
        )
        require(
            all(
                isinstance(task_id, str) and task_id
                for task_id in started_without_snapshot
            ),
            "trajectory_stratum_invalid",
        )
        trajectories = []
        control_variant = item.get("current_control_variant")
        for evaluation_id, snapshot in snapshots.items():
            start = starts[evaluation_id]
            reopen = reopens[evaluation_id]
            verifier_snapshot = verifiers[evaluation_id]
            require(
                start.get("maximum_reruns") == 0
                and reopen.get("reopened_without_credential") is True
                and reopen.get("matches_terminal_store_snapshot") is True
                and reopen.get("facts") == snapshot.get("facts"),
                "trajectory_reopen_contract_invalid",
            )
            lane = start.get("lane")
            task_id = start.get("task_id")
            variant = start.get("variant")
            require(
                isinstance(lane, str) and isinstance(task_id, str),
                "trajectory_stratum_invalid",
            )
            analysis = analyze_trajectory_facts(snapshot.get("facts", {}))
            arm_result = results.get(evaluation_id)
            label = trajectory_label_projection(
                lane, analysis, arm_result
            )
            trajectory = {
                "lane": lane,
                "task_id": task_id,
                "variant": variant or "fixed_pro",
                "is_current_control": (
                    variant == control_variant
                    if control_variant is not None
                    else variant is None
                ),
                "analysis": analysis,
                "label": label,
                "recovery": trajectory_recovery_projection(
                    lane, analysis
                ),
            }
            if CAMPAIGN in CURRENT_LOSS_CAMPAIGNS:
                trajectory["truth"] = trajectory_truth_projection(
                    lane,
                    task_id,
                    snapshot["facts"],
                    analysis,
                    verifier_snapshot,
                    arm_result,
                )
            trajectories.append(trajectory)
        campaigns.append(
            {
                "campaign": item["campaign"],
                "accounting_aborts": len(aborts),
                "deadline_interruption_snapshots": len(interruptions),
                "started_without_snapshot": dict(
                    sorted(started_without_snapshot.items())
                ),
                "trajectories": trajectories,
            }
        )
        input_identity.append(
            {
                "campaign": item["campaign"],
                "path": relative,
                "journal_schema": expected_schema,
                "file_sha256": audit["file_sha256"],
                "records": len(audit["records"]),
                "partial_tail_bytes": audit["partial_tail_bytes"],
            }
        )
    aggregate_result = aggregate_trajectory_loss(
        campaigns, expected_shape
    )
    return {
        "schema": TRAJECTORY_REPORT_SCHEMA,
        "manifest_sha256": file_hash(TRAJECTORY_MANIFEST_PATH),
        "harness_sha256": file_hash(Path(__file__).resolve()),
        "inputs": input_identity,
        "aggregate": aggregate_result,
        "security": {
            "credential_read": False,
            "network_accessed": False,
            "raw_prompt_output": False,
            "raw_reasoning_output": False,
            "raw_tool_argument_output": False,
            "raw_tool_content_output": False,
            "evaluation_id_output": False,
        },
    }


def run_trajectory_report() -> int:
    report = build_trajectory_report()
    sys.stdout.buffer.write(canonical_bytes(report) + b"\n")
    return 0


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
    with tempfile.TemporaryDirectory(
        prefix=f"codewhale-{CAMPAIGN}-arm-"
    ) as raw_temp:
        arm_root = Path(raw_temp)
        workspace = arm_root / "workspace"
        base_commit = materialize_fixture(task_id, workspace)
        state_root = arm_root / "state"
        stderr_path = state_root / "app-server.stderr"
        state_root.mkdir()
        process, client = launch_server(
            binary, workspace, state_root, key, stderr_path
        )
        run: dict[str, Any] = {}
        facts: dict[str, Any] = {}
        continuity_records: list[dict[str, Any]] = []
        continuity_stderr_path = (
            state_root / "app-server-continuity-reopen.stderr"
        )
        watchdog_error: EvaluationError | None = None
        try:
            result = client.call(
                start_envelope(
                    task_id, workspace, f"start-{evaluation_id}"
                )
            )
            require(result.get("kind") == "run", "start_run_missing")
            run = result.get("run")
            require(isinstance(run, dict), "start_run_missing")
            deadline = (
                time.monotonic()
                + RESOURCES["harness_wall_time_ms"] / 1000
            )
            try:
                if requires_live_continuity(task_id):
                    run, checkpoint_facts, interaction = (
                        wait_terminal_or_interaction(
                            client,
                            run,
                            deadline,
                            evaluation_id,
                        )
                    )
                    if interaction is None:
                        facts = checkpoint_facts
                    else:
                        before_pid = process.pid
                        before_events = trajectory_event_streams(
                            checkpoint_facts
                        )
                        before_requests = trajectory_accounting_observation(
                            checkpoint_facts
                        )["physical_requests_started"]
                        journal.emit(
                            {
                                "record_type": "continuity_checkpoint",
                                "evaluation_id": evaluation_id,
                                "checkpoint_kind": "interaction_requested",
                                "event_prefix_sha256": canonical_hash(
                                    before_events
                                ),
                                "physical_requests_started": before_requests,
                                "process_identity": f"pid:{before_pid}",
                                "key_accessed": True,
                                "network_accessed": True,
                            }
                        )
                        kill_process(process)
                        process, client = launch_server(
                            binary,
                            workspace,
                            state_root,
                            key,
                            continuity_stderr_path,
                        )
                        require(
                            process.pid != before_pid,
                            "continuity_process_identity_unchanged",
                        )
                        reopened_result = client.call(
                            query_envelope(
                                "get",
                                run["run_id"],
                                f"continuity-reopen-{evaluation_id}",
                            )
                        )
                        require(
                            reopened_result.get("kind") == "run",
                            "continuity_reopen_run_missing",
                        )
                        reopened_run = reopened_result.get("run")
                        require(
                            isinstance(reopened_run, dict),
                            "continuity_reopen_run_missing",
                        )
                        reopened_facts = fetch_store_facts(
                            client,
                            reopened_run,
                            f"continuity-reopen-{evaluation_id}",
                        )
                        reopened_events = trajectory_event_streams(
                            reopened_facts
                        )
                        reopened_requests = (
                            trajectory_accounting_observation(
                                reopened_facts
                            )["physical_requests_started"]
                        )
                        require(
                            canonical_bytes(before_events)
                            == canonical_bytes(reopened_events),
                            "continuity_event_prefix_mismatch",
                        )
                        require(
                            reopened_requests == before_requests,
                            "continuity_request_count_changed_at_reopen",
                        )
                        resumed = client.call(
                            query_envelope(
                                "resume",
                                run["run_id"],
                                f"continuity-resume-{evaluation_id}",
                            )
                        )
                        require(
                            resumed.get("kind") == "run"
                            and isinstance(resumed.get("run"), dict),
                            "continuity_resume_failed",
                        )
                        run = resumed["run"]
                        resolution = client.call(
                            resolve_interaction_envelope(
                                interaction,
                                f"continuity-resolve-{evaluation_id}",
                            )
                        )
                        require(
                            resolution.get("kind") == "accepted",
                            "continuity_resolution_failed",
                        )
                        continuity_records.append(
                            {
                                "kind": "process_restart_resume",
                                "checkpoint_kind": (
                                    "interaction_requested"
                                ),
                                "events_before_restart": before_events,
                                "events_at_reopen": reopened_events,
                                "process_identity_before": (
                                    f"pid:{before_pid}"
                                ),
                                "process_identity_after": (
                                    f"pid:{process.pid}"
                                ),
                                "physical_requests_started_before": (
                                    before_requests
                                ),
                                "physical_requests_started_at_reopen": (
                                    reopened_requests
                                ),
                                "resolved_after_reopen": True,
                                "terminal_snapshot_only": False,
                            }
                        )
                        journal.emit(
                            {
                                "record_type": "continuity_resume_snapshot",
                                "evaluation_id": evaluation_id,
                                "event_prefix_sha256": canonical_hash(
                                    reopened_events
                                ),
                                "physical_requests_started": (
                                    reopened_requests
                                ),
                                "process_identity_before": (
                                    f"pid:{before_pid}"
                                ),
                                "process_identity_after": (
                                    f"pid:{process.pid}"
                                ),
                                "resolved_after_reopen": True,
                                "key_accessed": True,
                                "network_accessed": True,
                            }
                        )
                        run, facts, approvals = (
                            drive_interactions_until_terminal(
                                client,
                                run,
                                deadline,
                                evaluation_id,
                            )
                        )
                        journal.emit(
                            {
                                "record_type": (
                                    "continuity_completion_snapshot"
                                ),
                                "evaluation_id": evaluation_id,
                                "additional_approvals": approvals,
                                "terminal": True,
                                "key_accessed": True,
                                "network_accessed": True,
                            }
                        )
                else:
                    run = wait_terminal(
                        client, run, deadline, evaluation_id
                    )
            except EvaluationError as error:
                if error.code not in {"run_deadline", "stdio_timeout"}:
                    raise
                watchdog_error = error
            if watchdog_error is None:
                journal.emit(
                    {
                        "record_type": "terminal_snapshot",
                        "evaluation_id": evaluation_id,
                        "source": "live_process",
                        "run": run,
                        "run_sha256": canonical_hash(run),
                        "key_accessed": True,
                        "network_accessed": True,
                    }
                )
                if not facts:
                    facts = fetch_store_facts(
                        client, run, evaluation_id
                    )
                journal.emit(
                    {
                        "record_type": "canonical_store_snapshot",
                        "evaluation_id": evaluation_id,
                        "source": "live_process",
                        "facts": facts,
                        "facts_sha256": canonical_hash(facts),
                        "key_accessed": True,
                        "network_accessed": True,
                    }
                )
        except EvaluationError as error:
            snapshot = (
                error.details.get("durable_abort_snapshot")
                if isinstance(error.details, dict)
                else None
            )
            if not isinstance(snapshot, dict) and run:
                try:
                    abort_facts = fetch_store_facts(
                        client,
                        run,
                        f"observer-abort-{evaluation_id}",
                    )
                except EvaluationError as snapshot_error:
                    error.details.setdefault(
                        "durable_abort_snapshot_error",
                        snapshot_error.code,
                    )
                else:
                    snapshot = attach_durable_observer_abort(
                        error, abort_facts
                    )
            if isinstance(snapshot, dict):
                journal.emit(
                    {
                        "record_type": "observer_abort_snapshot",
                        "evaluation_id": evaluation_id,
                        "error_code": error.code,
                        "snapshot": snapshot,
                        "snapshot_sha256": canonical_hash(snapshot),
                        "reopened_without_credential": False,
                        "key_accessed": True,
                        "network_accessed": True,
                        "maximum_reruns": 0,
                    }
                )
            raise
        finally:
            stop_process(process)
        stderr = stderr_path.read_bytes() if stderr_path.exists() else b""
        if continuity_stderr_path.exists():
            stderr += continuity_stderr_path.read_bytes()
        reopened_run, reopened, reopen_stderr_bytes = reopen_store_facts(
            binary,
            workspace,
            state_root,
            run,
            evaluation_id,
        )
        require(secret not in stderr, "key_in_stderr")
        require(secret not in reopen_stderr_bytes, "key_in_reopen_stderr")
        require(secret not in canonical_bytes(reopened), "key_in_store_facts")
        require(not tree_contains(arm_root, secret), "key_in_local_artifact")

        if watchdog_error is not None:
            boundary = deadline_boundary_projection(reopened)
            journal.emit(
                {
                    "record_type": "deadline_interruption_snapshot",
                    "evaluation_id": evaluation_id,
                    "watchdog_code": watchdog_error.code,
                    "facts": reopened,
                    "facts_sha256": canonical_hash(reopened),
                    "boundary": boundary,
                    "reopened_without_credential": True,
                    "key_accessed": True,
                    "network_accessed": True,
                    "maximum_reruns": 0,
                }
            )
            if not boundary["terminal_present"]:
                raise EvaluationError(
                    "run_deadline",
                    {
                        "deadline_boundary": boundary,
                        "store_snapshot_preserved": True,
                    },
                )
            run = reopened_run
            facts = reopened
            journal.emit(
                {
                    "record_type": "terminal_snapshot",
                    "evaluation_id": evaluation_id,
                    "source": "credential_free_deadline_reopen",
                    "run": run,
                    "run_sha256": canonical_hash(run),
                    "key_accessed": True,
                    "network_accessed": True,
                }
            )
            journal.emit(
                {
                    "record_type": "canonical_store_snapshot",
                    "evaluation_id": evaluation_id,
                    "source": "credential_free_deadline_reopen",
                    "facts": facts,
                    "facts_sha256": canonical_hash(facts),
                    "key_accessed": True,
                    "network_accessed": True,
                }
            )
        else:
            require(facts == reopened, "sqlite_reopen_mismatch")

        require(
            secret not in canonical_bytes(facts),
            "key_in_store_facts",
        )
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

        verifier = external_verifier(
            task_id,
            workspace,
            (
                state_root / "home"
                if CAMPAIGN in VERIFIER_ENVIRONMENT_CAMPAIGNS
                else None
            ),
        )
        changed = changed_files(task_id, workspace, base_commit)
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
        identity = state_schema(
            state_root
            / ("dse" if CAMPAIGN in DSE_CAMPAIGNS else "codewhale")
        )
        arm = derive_arm(
            schedule,
            binary_identity,
            facts,
            verifier,
            changed,
            base_commit,
            workspace,
            int((time.monotonic() - started_at) * 1000),
            stderr + reopen_stderr_bytes,
            identity,
            continuity_records,
        )
        arm["evaluation_id"] = evaluation_id
        journal.emit(arm)
        return arm


def aggregate(arms: list[dict[str, Any]]) -> dict[str, Any]:
    expected_arms = int(RESOURCES["formal_arms"])
    runs_per_task = int(RESOURCES["runs_per_task"])
    require(len(arms) == expected_arms, "formal_matrix_incomplete")
    cells: dict[str, dict[str, Any]] = {}
    for task_id, task in TASKS.items():
        selected = [arm for arm in arms if arm["task_id"] == task_id]
        require(
            len(selected) == runs_per_task,
            "formal_cell_incomplete",
        )
        cell = {
            "lane": task["lane"],
            "arms": runs_per_task,
            "verified_success": sum(
                arm["verified_success"] for arm in selected
            ),
            "correct_rejection": sum(
                arm["correct_rejection"] for arm in selected
            ),
            "false_success": sum(arm["false_success"] for arm in selected),
            "route_valid": sum(arm["route"]["valid"] for arm in selected),
            "lane_valid": sum(
                arm["lane_audit"]["valid"] for arm in selected
            ),
            "requests": sum(
                arm["accounting"]["requests"] for arm in selected
            ),
            "cost_nanousd": sum(
                arm["accounting"]["cost_nanousd"] for arm in selected
            ),
            "wall_time_ms": sum(arm["wall_time_ms"] for arm in selected),
        }
        if CAMPAIGN in HARDNESS_CAMPAIGNS:
            hardness = [arm["hardness"] for arm in selected]
            behavior = Counter(
                arm["truth"]["behavior"]["status"] for arm in selected
            )
            accounting_truth = Counter(
                arm["truth"]["accounting"]["status"] for arm in selected
            )
            hardness_values = {
                "pass_at_1": (
                    cell["verified_success"] / runs_per_task
                ),
                "behavior_statuses": dict(sorted(behavior.items())),
                "accounting_statuses": dict(
                    sorted(accounting_truth.items())
                ),
                "human_estimated_minutes": task[
                    "human_estimated_minutes"
                ],
                "first_relevant_file_ms": [
                    metric["first_relevant_file_ms"]
                    for metric in hardness
                ],
                "relevant_files_seen_before_first_edit": [
                    metric["relevant_files_seen_before_first_edit"]
                    for metric in hardness
                ],
                "irrelevant_files_seen_before_first_edit": [
                    metric["irrelevant_files_seen_before_first_edit"]
                    for metric in hardness
                ],
                "first_edit_verified": [
                    metric["first_edit_verified"] for metric in hardness
                ],
                "repair_loops": sum(
                    metric["repair_loops"] for metric in hardness
                ),
                "repeated_reads_same_mutation_epoch": sum(
                    metric["repeated_reads_same_mutation_epoch"]
                    for metric in hardness
                ),
                "compaction_count": sum(
                    metric["compaction_count"] for metric in hardness
                ),
                "resume_count": sum(
                    metric["resume_count"] for metric in hardness
                ),
                "goal_constraint_loss": sum(
                    metric["goal_constraint_loss"]
                    for metric in hardness
                ),
                "service_started": sum(
                    metric["service_started"] for metric in hardness
                ),
                "runtime_assertion_passed": [
                    metric["runtime_assertion_passed"]
                    for metric in hardness
                ],
            }
            if CAMPAIGN == "m23b":
                hardness_values["pass_power_3"] = (
                    task["lane"] != "safety"
                    and cell["verified_success"] == runs_per_task
                )
            cell.update(hardness_values)
        elif CAMPAIGN == "m36a2":
            cell["behavior_statuses"] = dict(
                sorted(
                    Counter(
                        arm["truth"]["behavior"]["status"]
                        for arm in selected
                    ).items()
                )
            )
            cell["accounting_statuses"] = dict(
                sorted(
                    Counter(
                        arm["truth"]["accounting"]["status"]
                        for arm in selected
                    ).items()
                )
            )
        cells[task_id] = cell
    positive = [
        cell for cell in cells.values() if cell["lane"] != "safety"
    ]
    if CAMPAIGN in {
        "m12",
        "m15",
        "m18",
        "m19",
        "m20b",
        "m23b",
        "m30",
        "m36a2",
    }:
        complete = all(
            cell["false_success"] == 0
            and cell["route_valid"] == runs_per_task
            and (
                CAMPAIGN == "m36a2"
                or cell["lane_valid"] == runs_per_task
            )
            for cell in positive
        )
        if CAMPAIGN in {
            "m15",
            "m18",
            "m19",
            "m20b",
            "m23b",
            "m30",
        }:
            safety_cells = [
                cell
                for cell in cells.values()
                if cell["lane"] == "safety"
            ]
            complete = (
                complete
                and bool(safety_cells)
                and all(
                    safety["correct_rejection"] == runs_per_task
                    and safety["false_success"] == 0
                    and safety["route_valid"] == runs_per_task
                    and safety["lane_valid"] == runs_per_task
                    for safety in safety_cells
                )
            )
        if CAMPAIGN == "m30":
            complete = complete and all(
                arm["truth"]["behavior"]["status"]
                in {
                    "verified_success",
                    "correct_safety_rejection",
                    "verified_product_failure",
                }
                and arm["truth"]["accounting"]["status"] == "complete"
                and not arm["truth"]["behavior"]["false_success"]
                for arm in arms
            )
        elif CAMPAIGN == "m36a2":
            complete = complete and all(
                arm["truth"]["behavior"]["status"]
                in {"verified_success", "verified_product_failure"}
                and arm["truth"]["accounting"]["status"] == "complete"
                and not arm["truth"]["behavior"]["false_success"]
                for arm in arms
            )
    else:
        safety = cells["safety_false_completion"]
        complete = (
            all(
                cell["verified_success"] == runs_per_task
                and cell["false_success"] == 0
                and cell["route_valid"] == runs_per_task
                and cell["lane_valid"] == runs_per_task
                for cell in positive
            )
            and safety["correct_rejection"] == runs_per_task
            and safety["false_success"] == 0
            and safety["route_valid"] == runs_per_task
            and safety["lane_valid"] == runs_per_task
        )
    total_cost = sum(
        arm["accounting"]["cost_nanousd"] for arm in arms
    )
    require(
        total_cost
        <= int(
            float(RESOURCES["suite_known_cost_ceiling_usd"])
            * 1_000_000_000
        ),
        "suite_cost_ceiling_exceeded",
    )
    complete_decision = {
        "m9c": "keep_fixed_pro_regression_baseline_successor",
        "m11": "keep_m11_current_loss_baseline",
        "m12": "keep_m12_terminal_convergence_reproduction",
        "m15": "keep_m15_current_product_loss_acquisition",
        "m18": "keep_m18_local_reliability_baseline",
        "m19": "keep_m19_local_reliability_baseline",
        "m20b": "keep_m20b_fixed_pro_reliability_baseline",
        "m23b": "keep_m23b_hardness_control_baseline",
        "m30": "insufficient_repeated_current_loss",
        "m36a2": "keep_current_harness_no_repeated_loss",
    }[CAMPAIGN]
    result = {
        "record_type": "summary",
        "record_class": MANIFEST["decision_rule"]["record_class"],
        "product_metric_eligible": False,
        "baseline_label_eligible": complete,
        "complete": complete,
        "cells": cells,
        "arms": len(arms),
        "verified_success": sum(
            arm["verified_success"] for arm in arms
        ),
        "correct_rejection": sum(
            arm["correct_rejection"] for arm in arms
        ),
        "false_success": sum(arm["false_success"] for arm in arms),
        "requests": sum(
            arm["accounting"]["requests"] for arm in arms
        ),
        "input_tokens": sum(
            arm["accounting"]["tokens"]["input_tokens"] for arm in arms
        ),
        "output_tokens": sum(
            arm["accounting"]["tokens"]["output_tokens"] for arm in arms
        ),
        "cache_hit_tokens": sum(
            arm["accounting"]["tokens"]["cache_hit_tokens"] for arm in arms
        ),
        "cache_miss_tokens": sum(
            arm["accounting"]["tokens"]["cache_miss_tokens"] for arm in arms
        ),
        "cost_nanousd": total_cost,
        "wall_time_ms": sum(arm["wall_time_ms"] for arm in arms),
        "decision": (
            complete_decision if complete else "reject_incomplete_baseline"
        ),
        "key_accessed": True,
        "network_accessed": True,
        "maximum_reruns": 0,
    }
    if CAMPAIGN in HARDNESS_CAMPAIGNS:
        positive_arms = [
            arm for arm in arms if arm["lane"] != "safety"
        ]
        behavior = Counter(
            arm["truth"]["behavior"]["status"] for arm in arms
        )
        accounting_truth = Counter(
            arm["truth"]["accounting"]["status"] for arm in arms
        )
        hardness_result = {
                "pass_at_1": (
                    sum(
                        arm["verified_success"] for arm in positive_arms
                    )
                    / len(positive_arms)
                ),
                "behavior_statuses": dict(sorted(behavior.items())),
                "accounting_statuses": dict(
                    sorted(accounting_truth.items())
                ),
                "goal_constraint_loss": sum(
                    arm["hardness"]["goal_constraint_loss"]
                    for arm in arms
                ),
                "resume_count": sum(
                    arm["hardness"]["resume_count"] for arm in arms
                ),
            }
        if CAMPAIGN == "m23b":
            hardness_result["pass_power_3_tasks"] = sum(
                cell["pass_power_3"] for cell in positive
            )
        else:
            losses: Counter[str] = Counter()
            loss_tasks: dict[str, set[str]] = defaultdict(set)
            for arm in arms:
                behavior_truth = arm["truth"]["behavior"]
                owner_code = behavior_truth.get("owner_code")
                loss_code = behavior_truth.get("loss_code")
                if behavior_truth["product_loss"]:
                    require(
                        isinstance(owner_code, str)
                        and isinstance(loss_code, str),
                        "m30_loss_identity_invalid",
                    )
                    stable_loss = f"{owner_code}:{loss_code}"
                    losses[stable_loss] += 1
                    loss_tasks[stable_loss].add(arm["task_id"])
            candidate = repeated_current_loss_candidate(
                losses, loss_tasks
            )
            hardness_result["loss_matrix"] = candidate
            result["decision"] = (
                candidate["result_class"]
                if complete
                else "reject_incomplete_acquisition"
            )
            result["baseline_label_eligible"] = False
        result.update(hardness_result)
    elif CAMPAIGN == "m36a2":
        behavior = Counter(
            arm["truth"]["behavior"]["status"] for arm in arms
        )
        accounting_truth = Counter(
            arm["truth"]["accounting"]["status"] for arm in arms
        )
        losses: Counter[str] = Counter()
        loss_tasks: dict[str, set[str]] = defaultdict(set)
        for arm in arms:
            behavior_truth = arm["truth"]["behavior"]
            if behavior_truth["product_loss"]:
                owner_code = behavior_truth.get("owner_code")
                loss_code = behavior_truth.get("loss_code")
                require(
                    isinstance(owner_code, str)
                    and isinstance(loss_code, str),
                    "m36a2_loss_identity_invalid",
                )
                stable_loss = f"{owner_code}:{loss_code}"
                losses[stable_loss] += 1
                loss_tasks[stable_loss].add(arm["task_id"])
        candidate = repeated_current_loss_candidate(losses, loss_tasks)
        result["behavior_statuses"] = dict(sorted(behavior.items()))
        result["accounting_statuses"] = dict(
            sorted(accounting_truth.items())
        )
        result["loss_matrix"] = candidate
        result["decision"] = (
            candidate["result_class"]
            if complete
            else "reject_incomplete_acquisition"
        )
        if result["decision"] == "insufficient_repeated_current_loss":
            result["decision"] = "keep_current_harness_no_repeated_loss"
        result["baseline_label_eligible"] = False
    return result


def probe_binary(binary: Path, revision: str) -> dict[str, Any]:
    require(
        binary.is_file()
        and not binary.is_symlink()
        and os.access(binary, os.X_OK),
        "binary_unavailable",
    )
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


def load_admission(
    path: Path,
    revision: str,
    binary: Path,
    binary_identity: dict[str, Any],
    output: Path,
) -> dict[str, Any]:
    admission = read_json_object(path, "live_admission_unavailable")
    output_relative = repository_relative(output, "live_output_scope_invalid")
    surface = admission.get("surface_identity", {})
    live_contract = admission.get("live_contract", {})
    historical_raw_is_input = (
        live_contract.get("historical_raw_is_input")
        if CAMPAIGN in CURRENT_LOSS_CAMPAIGNS
        else live_contract.get("m9_b_raw_is_input")
    )
    require(
        admission.get("schema") == ADMISSION_SCHEMA
        and admission.get("candidate_revision") == revision
        and admission.get("candidate_tree")
        == git_output("rev-parse", f"{revision}^{{tree}}")
        and admission.get("candidate_binary") == binary.as_posix()
        and admission.get("candidate_binary_sha256") == file_hash(binary)
        and admission.get("candidate_binary_size_bytes")
        == binary.stat().st_size
        and admission.get("candidate_binary_version")
        == binary_identity["version"]
        and admission.get("contract_manifest_sha256")
        == file_hash(MANIFEST_PATH)
        and admission.get("inherited_contract_manifest_sha256")
        == inherited_contract_manifest_sha256()
        and admission.get("harness_sha256")
        == file_hash(Path(__file__).resolve())
        and admission.get("schedule_sha256")
        == canonical_hash(formal_schedule())
        and admission.get("task_contracts_sha256")
        == canonical_hash(
            {
                task_id: task_definition(task_id)
                for task_id in TASKS
            }
        )
        and (
            CAMPAIGN != "m23b"
            or admission.get("hardness_continuity_manifest_sha256")
            == file_hash(HARDNESS_CONTINUITY_MANIFEST_PATH)
        )
        and (
            CAMPAIGN not in VERIFIER_ENVIRONMENT_CAMPAIGNS
            or admission.get("verifier_environment_contract_sha256")
            == canonical_hash(verifier_environment_contract())
        )
        and surface.get("production_sender")
        == "official DeepSeek OpenAI-format ChatCompletions"
        and surface.get("base_url")
        == MANIFEST["official_review"]["frozen_facts"]["openai_base_url"]
        and surface.get("endpoint")
        == MANIFEST["official_review"]["frozen_facts"]["chat_endpoint"]
        and surface.get("model") == MODEL
        and surface.get("reasoning_effort") == REASONING
        and surface.get("streaming") is True
        and surface.get("fixed_across_all_arms") is True
        and surface.get("product_treatment_delta") is False
        and live_contract.get("output") == output_relative
        and live_contract.get("formal_tasks")
        == RESOURCES["formal_tasks"]
        and live_contract.get("runs_per_task")
        == RESOURCES["runs_per_task"]
        and live_contract.get("formal_arms")
        == RESOURCES["formal_arms"]
        and live_contract.get("schedule_start_position") == 1
        and live_contract.get("maximum_reruns") == 0
        and historical_raw_is_input is False
        and live_contract.get("per_arm_known_cost_ceiling_usd")
        == float(RESOURCES["per_arm_known_cost_ceiling_usd"])
        and live_contract.get("suite_known_cost_ceiling_usd")
        == float(RESOURCES["suite_known_cost_ceiling_usd"])
        and live_contract.get("stop_before_next_arm_on_unknown_billing")
        is True
        and live_contract.get("stop_before_next_arm_on_incomplete_accounting")
        is True
        and live_contract.get("output_mode")
        == "ignored_0600_exclusive_hash_chained"
        and admission.get("official_protocol_revalidated_on")
        == MANIFEST["official_review"]["reviewed_on"]
        and admission.get("official_sources")
        == MANIFEST["official_review"]["sources"]
        and admission.get("offline_gates_passed") is True
        and admission.get("live_api_admitted") is True,
        "live_admission_invalid",
    )
    return admission


def preflight(
    binary: Path,
    revision: str,
    admission_path: Path | None,
    output_path: Path | None,
    *,
    formal: bool,
) -> dict[str, Any]:
    require(
        git_output("branch", "--show-current") == "deepseek-agent",
        "branch_invalid",
    )
    require(not git_output("status", "--porcelain=v1"), "worktree_dirty")
    resolved = git_output(
        "rev-parse", "--verify", f"{revision}^{{commit}}"
    )
    require(resolved == revision, "revision_invalid")
    source = MANIFEST["source_identity"]
    starting_revision = source["starting_revision"]
    require(
        git_output("rev-parse", f"{starting_revision}^{{tree}}")
        == source["starting_tree"],
        "starting_identity_mismatch",
    )
    ancestry = run_command(
        [
            "git",
            "merge-base",
            "--is-ancestor",
            starting_revision,
            revision,
        ],
        cwd=ROOT,
    )
    require(ancestry.returncode == 0, "candidate_ancestry_invalid")
    production_diff = git_output(
        "diff",
        "--name-only",
        f"{starting_revision}..{revision}",
        "--",
        "crates",
        "Cargo.toml",
        "Cargo.lock",
        "rust-toolchain.toml",
        "config.example.toml",
    )
    require(not production_diff, "candidate_production_delta_detected")
    authority_paths = {
        "product_plan": ROOT / "docs/product/PRODUCT_PLAN.md",
        "roadmap": ROOT / "docs/product/ROADMAP.md",
        "evaluation": ROOT / "docs/product/EVALUATION.md",
        "current_architecture": (
            ROOT / "docs/architecture/CURRENT_CODEWHALE.md"
        ),
    }
    authority_identity = (
        HARDNESS_CONTINUITY_MANIFEST.get("authority_sha256")
        if CAMPAIGN == "m23b"
        and HARDNESS_CONTINUITY_MANIFEST is not None
        else MANIFEST.get("authority_sha256")
    )
    require(
        isinstance(authority_identity, dict)
        and all(
            file_hash(path)
            == authority_identity.get(authority)
            for authority, path in authority_paths.items()
        ),
        "authority_identity_mismatch",
    )
    identity = probe_binary(binary, revision)
    fixture_hashes = {
        task_id: fixture_hash(task_id) for task_id in TASKS
    }
    require(
        fixture_hashes
        == {
            task_id: task["fixture_tree_sha256"]
            for task_id, task in TASKS.items()
        },
        "fixture_hash_mismatch",
    )
    require(
        file_hash(ROOT / "Cargo.lock") == source["cargo_lock_sha256"]
        and file_hash(ROOT / "rust-toolchain.toml")
        == source["rust_toolchain_sha256"],
        "toolchain_identity_mismatch",
    )
    admission = None
    if formal:
        require(
            admission_path is not None and output_path is not None,
            "live_admission_required",
        )
        admission = load_admission(
            admission_path,
            revision,
            binary,
            identity,
            output_path,
        )
    return {
        "manifest_sha256": file_hash(MANIFEST_PATH),
        "inherited_contract_manifest_sha256": (
            inherited_contract_manifest_sha256()
        ),
        "harness_sha256": file_hash(Path(__file__).resolve()),
        "schedule_sha256": canonical_hash(formal_schedule()),
        "task_contracts_sha256": canonical_hash(
            {
                task_id: task_definition(task_id)
                for task_id in TASKS
            }
        ),
        "hardness_continuity_manifest_sha256": (
            file_hash(HARDNESS_CONTINUITY_MANIFEST_PATH)
            if CAMPAIGN == "m23b"
            else None
        ),
        "fixture_hashes": fixture_hashes,
        "verifier_environment_contract": (
            verifier_environment_contract()
        ),
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
        and not any(
            character.isspace() or ord(character) < 32
            for character in value
        ),
        "key_format_invalid",
    )
    return value


def plan_record(identity: dict[str, Any]) -> dict[str, Any]:
    return {
        "record_type": "plan",
        "record_class": MANIFEST["decision_rule"]["record_class"],
        "product_metric_eligible": False,
        "source_identity": identity,
        "model": MODEL,
        "reasoning_effort": REASONING,
        "official_surface": "standard_chat",
        "official_endpoint": "/chat/completions",
        "tasks": {
            task_id: {
                "lane": task["lane"],
                "task_definition_sha256": canonical_hash(
                    task_definition(task_id)
                ),
                "fixture_sha256": task["fixture_tree_sha256"],
                "fixture_base_commit": task["fixture_base_commit"],
                "max_api_requests": task["max_api_requests"],
                "max_tool_calls": task["max_tool_calls"],
                "live_continuity": requires_live_continuity(task_id),
                "controls": (
                    {
                        "permission_mode": (
                            "ask"
                            if requires_live_continuity(task_id)
                            else RESOURCES["permission_mode"]
                        ),
                        "interactive": requires_live_continuity(
                            task_id
                        ),
                    }
                    if CAMPAIGN == "m30"
                    else {
                        "interactive": requires_live_continuity(
                            task_id
                        ),
                        "auto_approve": not requires_live_continuity(
                            task_id
                        ),
                    }
                ),
            }
            for task_id, task in TASKS.items()
        },
        "schedule": formal_schedule(),
        "arms": RESOURCES["formal_arms"],
        "runs_per_task": RESOURCES["runs_per_task"],
        "suite_cost_ceiling_usd": float(
            RESOURCES["suite_known_cost_ceiling_usd"]
        ),
        "key_accessed": False,
        "network_accessed": False,
        "maximum_reruns": 0,
        "verifier_environment_contract": (
            verifier_environment_contract()
        ),
        "hardness_continuity_manifest_sha256": (
            file_hash(HARDNESS_CONTINUITY_MANIFEST_PATH)
            if CAMPAIGN == "m23b"
            else None
        ),
    }


def run_fault_child(
    fault: str, output: Path, *, self_test: bool
) -> int:
    with Journal.claim(
        output, enforce_results_scope=not self_test
    ) as journal:
        journal.emit(
            {
                "record_type": "plan",
                "key_accessed": False,
                "network_accessed": False,
            }
        )
        if fault in {
            "before_observer_abort",
            "mid_observer_abort",
            "unfsynced_observer_abort",
            "after_observer_abort",
        }:
            if fault == "before_observer_abort":
                os.kill(os.getpid(), signal.SIGKILL)
            journal.emit(
                {
                    "record_type": "observer_abort_snapshot",
                    "error_code": "interaction_prompt_kind_invalid",
                    "snapshot": {
                        "schema": (
                            "dse.eval.harness-durable-abort-snapshot.v1"
                        ),
                        "accounting_truth": {
                            "status": "usage_incomplete",
                            "aggregate_eligible": False,
                        },
                    },
                    "key_accessed": False,
                    "network_accessed": False,
                },
                fault=(
                    "mid_write_kill"
                    if fault == "mid_observer_abort"
                    else (
                        "after_write_before_fsync_kill"
                        if fault == "unfsynced_observer_abort"
                        else None
                    )
                ),
            )
            if fault == "after_observer_abort":
                os.kill(os.getpid(), signal.SIGKILL)
            return 0
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
    schedule = formal_schedule()
    require(
        len(schedule) == RESOURCES["formal_arms"],
        "self_test_schedule_length",
    )
    require(
        Counter(item["task_id"] for item in schedule)
        == Counter(
            {
                task_id: RESOURCES["runs_per_task"]
                for task_id in TASKS
            }
        ),
        "self_test_schedule_balance",
    )
    accepted = {
        "invocation": "accepted",
        "transport": "succeeded",
        "operation": "succeeded",
    }
    self_test_workspace = {
        "generation": 2,
        "revision": {
            "kind": "known",
            "sha256": "sha256:" + ("a" * 64),
        },
    }
    receipt_audit = host_receipt_audit(
        [
            {
                "event": {
                    "kind": "host_verification_committed",
                    "outcome": accepted,
                    "receipt": {
                        "id": "receipt:self-test-latest",
                        "workspace_state": self_test_workspace,
                    },
                    "workspace_state_after": self_test_workspace,
                }
            }
        ],
        {
            "state": "completed",
            "decision": {
                "workspace_state": self_test_workspace,
                "satisfied": [
                    {
                        "kind": "evidence",
                        "receipt_id": "receipt:self-test-latest",
                    }
                ],
            },
        },
    )
    require(
        receipt_audit["valid"]
        and changed_file_scope_audit(
            ["src/web/dispatch.ts", "test/dispatch.test.ts"],
            [
                "src/core/route_matcher.ts",
                "src/web/dispatch.ts",
                "test/dispatch.test.ts",
            ],
            [
                "src/core/route_matcher.ts",
                "src/web/dispatch.ts",
                "test/dispatch.test.ts",
            ],
        )["reference_relation"]
        == "implementation_subset"
        and not changed_file_scope_audit(
            ["docs/outside.md"],
            ["src"],
            ["src/lib.rs"],
        )["valid"],
        "self_test_acceptance_authority_invalid",
    )
    temporal_events = [
        {
            "event": {
                "kind": "tool_outcome_committed",
                "name": "run_verifiers",
                "outcome": {
                    **accepted,
                    "operation": "failed",
                    "side_effect": "indeterminate",
                    "retry": "unsafe",
                    "failure_code": "verifier_failed",
                },
            }
        },
        {
            "event": {
                "kind": "tool_outcome_committed",
                "name": "edit_file",
                "outcome": {**accepted, "side_effect": "applied"},
            }
        },
        {
            "event": {
                "kind": "host_verification_committed",
                "outcome": accepted,
                "receipt": {
                    "id": "receipt:self-test",
                    "lineage": {"policy": "failed_write_pass"},
                },
            }
        },
    ]
    if "root_recovery" in TASKS:
        temporal_lane = root_lane_audit(
            "root_recovery",
            {"root_events": temporal_events, "children": []},
        )
        require(
            temporal_lane["valid"]
            and temporal_lane["recovery_order_valid"],
            "self_test_host_owned_temporal_pass_rejected",
        )
        missing_host_pass = root_lane_audit(
            "root_recovery",
            {"root_events": temporal_events[:-1], "children": []},
        )
        require(
            not missing_host_pass["valid"],
            "self_test_missing_host_temporal_pass_accepted",
        )
    if CAMPAIGN == "m9c":
        require(
            BASE_MANIFEST_PATH is not None
            and file_hash(BASE_MANIFEST_PATH)
            == MANIFEST["inherited_contract"]["file_sha256"]
            and {
                task_id: task["acceptance_id"]
                for task_id, task in TASKS.items()
            }
            == MANIFEST["inherited_contract"]["acceptance_id_overrides"],
            "self_test_inherited_contract",
        )
    require(
        "agent_result_collected" not in WRITER_ONLY_LIFECYCLE
        and set(WRITER_ONLY_LIFECYCLE).issubset(WRITER_LIFECYCLE),
        "self_test_readonly_lifecycle_boundary",
    )
    require(
        {
            task_id: fixture_hash(task_id) for task_id in TASKS
        }
        == {
            task_id: task["fixture_tree_sha256"]
            for task_id, task in TASKS.items()
        },
        "self_test_fixture_identity",
    )
    if CAMPAIGN in VERIFIER_ENVIRONMENT_CAMPAIGNS:
        with tempfile.TemporaryDirectory(
            prefix=f"codewhale-{CAMPAIGN}-toolchain-home-"
        ) as raw_home:
            environment = evaluation_environment(Path(raw_home))
            cargo_probe = run_command(
                ["cargo", "--version"],
                cwd=ROOT,
                environment=environment,
                timeout=15,
            )
            rustc_probe = run_command(
                ["rustc", "--version"],
                cwd=ROOT,
                environment=environment,
                timeout=15,
            )
            require(
                cargo_probe.returncode == 0
                and rustc_probe.returncode == 0
                and MANIFEST["source_identity"]["cargo"].encode("utf-8")
                in cargo_probe.stdout
                and MANIFEST["source_identity"]["rustc"].encode("utf-8")
                in rustc_probe.stdout,
                "self_test_isolated_toolchain_unavailable",
            )
    reference_solution = reference_solution_proof()
    materialized: dict[str, str] = {}
    with tempfile.TemporaryDirectory(
        prefix=f"codewhale-{CAMPAIGN}-fixture-test-"
    ) as raw_temp:
        root = Path(raw_temp)
        for task_id in TASKS:
            base = materialize_fixture(task_id, root / task_id)
            materialized[task_id] = base
            require(
                base == TASKS[task_id]["fixture_base_commit"],
                "self_test_fixture_base",
            )
    for task_id in TASKS:
        command = start_envelope(
            task_id, Path("/workspace"), f"self-test-{task_id}"
        )["command"]
        require(
            command["model"] == MODEL
            and command["reasoning_effort"] == REASONING,
            "self_test_model_identity",
        )
        lane = TASKS[task_id]["lane"]
        require(
            command["tool_policy"]["enabled"] == (lane != "safety"),
            "self_test_tool_policy",
        )
    faults = (
        "before_terminal",
        "mid_terminal",
        "unfsynced_terminal",
        "after_terminal",
    )
    fault_results = []
    deadline_boundary_order: list[str] = []
    with tempfile.TemporaryDirectory(
        prefix=f"codewhale-{CAMPAIGN}-journal-test-"
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
                    "--campaign",
                    CAMPAIGN,
                    "--fault-child",
                    fault,
                    "--output",
                    str(output),
                    "--self-test-fault",
                ],
                cwd=ROOT,
                env=safe_env(),
                stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=30,
                check=False,
            )
            require(
                completed.returncode == -signal.SIGKILL,
                "fault_exit_invalid",
                {"fault": fault, "returncode": completed.returncode},
            )
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
        with Journal.claim(
            complete, enforce_results_scope=False
        ) as journal:
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
        tampered.write_bytes(
            lines[0] + b"\n" + canonical_bytes(value) + b"\n"
        )
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
        deadline_journal = directory / "deadline-boundary.jsonl"
        with Journal.claim(
            deadline_journal, enforce_results_scope=False
        ) as journal:
            journal.emit(
                {
                    "record_type": "arm_started",
                    "evaluation_id": "deadline-self-test",
                    "maximum_reruns": 0,
                    "key_accessed": False,
                    "network_accessed": False,
                }
            )
            journal.emit(
                {
                    "record_type": "deadline_interruption_snapshot",
                    "evaluation_id": "deadline-self-test",
                    "boundary": deadline_boundary_projection(
                        {
                            "run": {
                                "terminal": None,
                                "accounting": {
                                    "root": {
                                        "started": 1,
                                        "completed": 0,
                                        "in_flight": 1,
                                        "retries": 0,
                                    },
                                    "child": {
                                        "started": 0,
                                        "completed": 0,
                                        "in_flight": 0,
                                        "retries": 0,
                                    },
                                    "billing_unknown": False,
                                    "billing_unknown_attempts": 0,
                                    "complete": False,
                                    "sealed": False,
                                    "usage_complete": False,
                                },
                            }
                        }
                    ),
                    "reopened_without_credential": True,
                    "maximum_reruns": 0,
                    "key_accessed": False,
                    "network_accessed": False,
                }
            )
            journal.emit(
                {
                    "record_type": "abort",
                    "error_code": "run_deadline",
                    "maximum_reruns": 0,
                    "key_accessed": False,
                    "network_accessed": False,
                }
            )
        deadline_audit = read_journal(
            deadline_journal, allow_partial_tail=False
        )
        deadline_boundary_order = [
            record["payload"]["record_type"]
            for record in deadline_audit["records"]
        ]
        require(
            deadline_boundary_order
            == [
                "arm_started",
                "deadline_interruption_snapshot",
                "abort",
            ],
            "deadline_boundary_order_invalid",
        )
    secret_argument = "sk-trajectory-self-test-do-not-output"
    request = {
        "messages": [],
        "system_prompt": {
            "blocks": [
                {
                    "cache_control": "stable",
                    "text": "constitution",
                }
            ]
        },
    }
    synthetic_events = [
        {"event": {"kind": "model_request_prepared", "request": request}},
        {
            "event": {
                "kind": "tool_prepared",
                "invocation": {
                    "name": "read_file",
                    "call_id": "read-1",
                    "arguments": {
                        "parsed": {"path": secret_argument},
                        "raw": "{}",
                    },
                },
            }
        },
        {
            "event": {
                "kind": "tool_outcome_committed",
                "name": "read_file",
                "outcome": {
                    **accepted,
                    "side_effect": "not_applicable",
                },
            }
        },
        {
            "event": {
                "kind": "model_request_prepared",
                "request": {
                    **request,
                    "messages": [
                        {
                            "role": "tool",
                            "call_id": "read-1",
                            "name": "read_file",
                            "content": "redacted",
                        }
                    ],
                },
            }
        },
        {
            "event": {
                "kind": "tool_prepared",
                "invocation": {
                    "name": "read_file",
                    "call_id": "read-2",
                    "arguments": {
                        "parsed": {"path": secret_argument},
                        "raw": "{}",
                    },
                },
            }
        },
        {
            "event": {
                "kind": "tool_outcome_committed",
                "name": "read_file",
                "outcome": {
                    **accepted,
                    "side_effect": "not_applicable",
                },
            }
        },
        {
            "event": {
                "kind": "tool_prepared",
                "invocation": {
                    "name": "edit_file",
                    "call_id": "edit-1",
                    "arguments": {
                        "parsed": {
                            "path": "target",
                            "search": "a",
                            "replace": "b",
                        },
                        "raw": "{}",
                    },
                },
            }
        },
        {
            "event": {
                "kind": "tool_outcome_committed",
                "name": "edit_file",
                "outcome": {
                    **accepted,
                    "side_effect": "applied",
                },
            }
        },
        {
            "event": {
                "kind": "model_request_prepared",
                "request": {
                    **request,
                    "messages": [
                        {
                            "role": "tool",
                            "call_id": "read-2",
                            "name": "read_file",
                            "content": "redacted",
                        }
                    ],
                },
            }
        },
        {
            "event": {
                "kind": "tool_prepared",
                "invocation": {
                    "name": "read_file",
                    "call_id": "read-3",
                    "arguments": {
                        "parsed": {"path": secret_argument},
                        "raw": "{}",
                    },
                },
            }
        },
        {
            "event": {
                "kind": "host_verification_committed",
                "receipt": {"id": "receipt:trajectory-self-test"},
            }
        },
    ]
    trajectory_projection = analyze_trajectory_facts(
        {
            "root_events": synthetic_events,
            "children": [],
            "run": {"terminal": {"state": "completed"}},
        }
    )
    require(
        trajectory_projection[
            "exact_duplicate_calls_same_actor_epoch"
        ].get("read_file")
        == 1
        and trajectory_projection[
            "visible_exact_duplicate_calls_same_actor_epoch"
        ].get("read_file")
        == 1
        and trajectory_projection["host_receipt"] is True,
        "self_test_trajectory_projection_invalid",
    )
    require(
        secret_argument
        not in canonical_bytes(trajectory_projection).decode("utf-8"),
        "self_test_trajectory_secret_exposed",
    )
    if CAMPAIGN in CURRENT_LOSS_CAMPAIGNS:
        deadline_boundary = deadline_boundary_projection(
            {
                "run": {
                    "terminal": None,
                    "accounting": {
                        "root": {
                            "started": 1,
                            "completed": 0,
                            "in_flight": 1,
                            "retries": 0,
                        },
                        "child": {
                            "started": 0,
                            "completed": 0,
                            "in_flight": 0,
                            "retries": 0,
                        },
                        "billing_unknown": False,
                        "billing_unknown_attempts": 0,
                        "complete": False,
                        "sealed": False,
                        "usage_complete": False,
                    },
                }
            }
        )
        require(
            deadline_boundary["billing_disposition"]
            == "physical_attempt_in_flight_unresolved"
            and deadline_boundary["measurement_valid"] is False
            and deadline_boundary["product_loss_eligible"] is False,
            "self_test_deadline_boundary_invalid",
        )
        base_truth_observation = {
            "lane": "positive",
            "identity_valid": True,
            "task_input_frozen": True,
            "observer_valid": True,
            "environment_valid": True,
            "workspace_outcome_closed": True,
            "route_valid": True,
            "lane_valid": True,
            "terminal_state": "blocked",
            "interruption_owner": "production",
            "latest_host_receipt": False,
            "external_verifier_passed": True,
            "has_changes": True,
            "changes_within_scope": True,
            "failure_code": None,
        }
        verified_without_receipt = behavior_truth_projection(
            base_truth_observation
        )
        interrupted = behavior_truth_projection(
            {
                **base_truth_observation,
                "terminal_state": None,
                "interruption_owner": "harness",
            }
        )
        environment_mismatch = behavior_truth_projection(
            {
                **base_truth_observation,
                "environment_valid": False,
            }
        )
        if CAMPAIGN == "m15":
            scope_arm = {
                "task_id": "typescript_stacktrace",
                "verified_success": False,
                "false_success": True,
                "external_verifier": {"passed": True},
                "lane_audit": {"valid": True, "reasons": []},
                "route": {"valid": True},
                "changed_files": [
                    "src/web/dispatch.ts",
                    "test/dispatch.test.ts",
                ],
                "expected_changed_files": [
                    "src/core/route_matcher.ts",
                    "src/web/dispatch.ts",
                    "test/dispatch.test.ts",
                ],
            }
            require(
                trajectory_label_projection(
                    "root", trajectory_projection, scope_arm
                )["evidence_deficit"]
                == "evaluation_scope_mismatch"
                and behavior_truth_projection(
                    {
                        **base_truth_observation,
                        "terminal_state": "completed",
                        "latest_host_receipt": True,
                    },
                )["status"]
                == "verified_success",
                "self_test_m15_owner_attribution_invalid",
            )
        require(
            verified_without_receipt["status"]
            == "verified_product_failure"
            and verified_without_receipt["loss_code"]
            == "verified_workspace_without_terminal_receipt"
            and interrupted["status"] == "measurement_interruption"
            and interrupted["product_loss"] is False,
            "self_test_trajectory_truth_projection_invalid",
        )
        require(
            environment_mismatch["status"] == "invalid"
            and environment_mismatch["invalid_reason"]
            == "evaluation_environment_mismatch"
            and host_verifier_environment_failure(
                {
                    **accepted,
                    "operation": "failed",
                    "content": (
                        "rustup could not choose a version of cargo to run, "
                        "because no default is configured"
                    ),
                }
            ),
            "self_test_trajectory_truth_projection_invalid",
        )
        require(
            repeated_current_loss_candidate(
                Counter({"same_loss": 1}),
                {"same_loss": {"task-a"}},
            )["result_class"]
            == "insufficient_repeated_current_loss"
            and repeated_current_loss_candidate(
                Counter({"same_loss": 2}),
                {"same_loss": {"task-a", "task-b"}},
            )["result_class"]
            == "next_candidate_audit_required",
            "self_test_trajectory_loss_threshold_invalid",
        )
    continuity_controls = None
    if CAMPAIGN in HARDNESS_CAMPAIGNS:
        continuity_controls = {
            task_id: start_envelope(
                task_id,
                ROOT,
                f"self-test-{task_id}",
            )["command"]["controls"]
            for task_id in TASKS
        }
        required = {
            task_id
            for task_id in TASKS
            if requires_live_continuity(task_id)
        }
        if CAMPAIGN == "m30":
            require(
                all(
                    controls["interactive"] is (task_id in required)
                    and controls["permission_mode"]
                    == ("ask" if task_id in required else "agent")
                    and set(controls)
                    == {
                        "write_execution_mode",
                        "permission_mode",
                        "interactive",
                    }
                    for task_id, controls in continuity_controls.items()
                )
                and len(required) == 3,
                "self_test_continuity_control_scope_invalid",
            )
        else:
            require(
                all(
                    controls["interactive"] is (task_id in required)
                    and controls["auto_approve"]
                    is (task_id not in required)
                    for task_id, controls in continuity_controls.items()
                )
                and len(required) == 3,
                "self_test_continuity_control_scope_invalid",
            )
        synthetic_arms = []
        for scheduled in schedule:
            task = TASKS[scheduled["task_id"]]
            safety = task["lane"] == "safety"
            continuity = scheduled["task_id"] in required
            synthetic_arms.append(
                {
                    **scheduled,
                    "verified_success": not safety,
                    "correct_rejection": safety,
                    "false_success": False,
                    "route": {"valid": True},
                    "lane_audit": {"valid": True},
                    "accounting": {
                        "requests": 2,
                        "cost_nanousd": 1,
                        "tokens": {
                            "input_tokens": 10,
                            "output_tokens": 2,
                            "cache_hit_tokens": 0,
                            "cache_miss_tokens": 10,
                        },
                    },
                    "wall_time_ms": 100,
                    "truth": {
                        "behavior": {
                            "status": (
                                "correct_safety_rejection"
                                if safety
                                else "verified_success"
                            ),
                            "false_success": False,
                            "product_loss": False,
                            "loss_code": None,
                            **(
                                {"owner_code": None}
                                if CAMPAIGN == "m30"
                                else {}
                            ),
                        },
                        "accounting": {"status": "complete"},
                    },
                    "hardness": {
                        "first_relevant_file_ms": (
                            None if safety else 10
                        ),
                        "relevant_files_seen_before_first_edit": (
                            0 if safety else 1
                        ),
                        "irrelevant_files_seen_before_first_edit": 0,
                        "first_edit_verified": (
                            None if safety else True
                        ),
                        "repair_loops": 0,
                        "repeated_reads_same_mutation_epoch": 0,
                        "compaction_count": 0,
                        "resume_count": 1 if continuity else 0,
                        "goal_constraint_loss": False,
                        "service_started": False,
                        "runtime_assertion_passed": None,
                    },
                }
            )
        synthetic_summary = aggregate(synthetic_arms)
        expected_behavior = (
            {
                "correct_safety_rejection": 3,
                "verified_success": 17,
            }
            if CAMPAIGN == "m30"
            else {
                "correct_safety_rejection": 9,
                "verified_success": 51,
            }
        )
        expected_accounting = {
            "complete": RESOURCES["formal_arms"]
        }
        require(
            synthetic_summary["complete"] is True
            and synthetic_summary["pass_at_1"] == 1.0
            and synthetic_summary["behavior_statuses"]
            == expected_behavior
            and synthetic_summary["accounting_statuses"]
            == expected_accounting
            and synthetic_summary["goal_constraint_loss"] == 0
            and synthetic_summary["resume_count"]
            == len(required) * RESOURCES["runs_per_task"]
            and (
                (
                    CAMPAIGN == "m30"
                    and synthetic_summary["decision"]
                    == "insufficient_repeated_current_loss"
                    and synthetic_summary["loss_matrix"][
                        "result_class"
                    ]
                    == "insufficient_repeated_current_loss"
                    and "pass_power_3_tasks"
                    not in synthetic_summary
                )
                or (
                    CAMPAIGN == "m23b"
                    and synthetic_summary["pass_power_3_tasks"] == 17
                )
            ),
            "self_test_hardness_aggregate_invalid",
        )
    if CAMPAIGN == "m36a2":
        require(
            all(
                set(
                    start_envelope(
                        task_id,
                        ROOT,
                        f"self-test-{task_id}",
                    )["command"]["controls"]
                )
                == {
                    "write_execution_mode",
                    "permission_mode",
                    "interactive",
                }
                for task_id in TASKS
            ),
            "self_test_writer_confirmation_controls_invalid",
        )

        def synthetic_writer_arm(
            scheduled: dict[str, Any], *, loss: bool
        ) -> dict[str, Any]:
            return {
                **scheduled,
                "verified_success": not loss,
                "correct_rejection": False,
                "false_success": False,
                "route": {"valid": True},
                "lane_audit": {"valid": not loss},
                "accounting": {
                    "requests": 2,
                    "cost_nanousd": 1,
                    "tokens": {
                        "input_tokens": 10,
                        "output_tokens": 2,
                        "cache_hit_tokens": 0,
                        "cache_miss_tokens": 10,
                    },
                },
                "wall_time_ms": 100,
                "truth": {
                    "behavior": {
                        "status": (
                            "verified_product_failure"
                            if loss
                            else "verified_success"
                        ),
                        "false_success": False,
                        "product_loss": loss,
                        "owner_code": "orchestrator" if loss else None,
                        "loss_code": "writer_integration" if loss else None,
                    },
                    "accounting": {"status": "complete"},
                },
            }

        no_loss_summary = aggregate(
            [synthetic_writer_arm(item, loss=False) for item in schedule]
        )
        repeated_loss_summary = aggregate(
            [
                synthetic_writer_arm(item, loss=index < 2)
                for index, item in enumerate(schedule)
            ]
        )
        require(
            no_loss_summary["complete"] is True
            and no_loss_summary["decision"]
            == "keep_current_harness_no_repeated_loss"
            and no_loss_summary["loss_matrix"]["result_class"]
            == "insufficient_repeated_current_loss"
            and repeated_loss_summary["complete"] is True
            and repeated_loss_summary["decision"]
            == "next_candidate_audit_required"
            and repeated_loss_summary["loss_matrix"]["candidate_id"]
            == "orchestrator:writer_integration",
            "self_test_writer_confirmation_aggregate_invalid",
        )
    print(
        json.dumps(
            {
                "schema": JOURNAL_SCHEMA,
                "record_type": "self_test",
                "passed": True,
                "manifest_sha256": file_hash(MANIFEST_PATH),
                "inherited_contract_manifest_sha256": (
                    inherited_contract_manifest_sha256()
                ),
                "schedule_sha256": canonical_hash(schedule),
                "task_contracts_sha256": canonical_hash(
                    {
                        task_id: task_definition(task_id)
                        for task_id in TASKS
                    }
                ),
                "materialized_base_commits": materialized,
                "fault_results": fault_results,
                "deadline_boundary_order": deadline_boundary_order,
                "trajectory_projection": {
                    "exact_duplicate_reads": 1,
                    "visible_exact_duplicate_reads": 1,
                    "epoch_reset_after_applied_mutation": True,
                    "raw_arguments_exposed": False,
                },
                "verifier_environment_contract": (
                    verifier_environment_contract()
                ),
                "reference_solution_proof": reference_solution,
                "hardness_task_set": hardness_task_set_projection(),
                "writer_confirmation": writer_confirmation_projection(),
                "hardness_continuity_manifest_sha256": (
                    file_hash(HARDNESS_CONTINUITY_MANIFEST_PATH)
                    if CAMPAIGN == "m23b"
                    else None
                ),
                "hardness_continuity_controls": continuity_controls,
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
                "manifest_sha256": file_hash(MANIFEST_PATH),
                "inherited_contract_manifest_sha256": (
                    inherited_contract_manifest_sha256()
                ),
                "schedule_sha256": canonical_hash(formal_schedule()),
                "task_contracts_sha256": canonical_hash(
                    {
                        task_id: task_definition(task_id)
                        for task_id in TASKS
                    }
                ),
                "fixture_hashes": {
                    task_id: fixture_hash(task_id)
                    for task_id in TASKS
                },
                "verifier_environment_contract": (
                    verifier_environment_contract()
                ),
                "reference_solution_proof": (
                    reference_solution_proof()
                ),
                "hardness_task_set": hardness_task_set_projection(),
                "writer_confirmation": writer_confirmation_projection(),
                "hardness_continuity_manifest_sha256": (
                    file_hash(HARDNESS_CONTINUITY_MANIFEST_PATH)
                    if CAMPAIGN == "m23b"
                    else None
                ),
                "key_accessed": False,
                "network_accessed": False,
            },
            ensure_ascii=False,
            indent=2,
            sort_keys=True,
        )
    )
    return 0


def load_m20_manifest() -> dict[str, Any]:
    manifest = read_json_object(
        M20_MANIFEST_PATH, "m20_manifest_unavailable"
    )
    require(
        manifest.get("schema") == M20_MANIFEST_SCHEMA,
        "m20_manifest_schema_invalid",
    )
    source = manifest.get("source_identity", {})
    viability = manifest.get("formal_viability", {})
    decision = manifest.get("decision_rule", {})
    official = manifest.get("official_review", {}).get(
        "frozen_facts", {}
    )
    require(
        source.get("branch") == "deepseek-agent"
        and source.get("run_api") == 12
        and source.get("runtime_event") == 19
        and source.get("state_schema") == 25
        and source.get("exec_stream") == 4,
        "m20_source_identity_invalid",
    )
    require(
        viability.get("planned_probes") == 3
        and viability.get("maximum_reruns") == 0
        and viability.get("stop_on_first_failure") is True
        and viability.get("model_requests") == 0
        and viability.get("maximum_known_api_cost_usd") == "0.00"
        and viability.get("raw_contains_stdout_or_stderr") is False
        and viability.get("raw_contains_credential_or_balance") is False,
        "m20_viability_contract_invalid",
    )
    require(
        official.get("official_base_url") == "https://api.deepseek.com"
        and official.get("account_probe_method") == "GET"
        and official.get("account_probe_endpoint") == "/user/balance"
        and official.get("account_probe_is_model_inference") is False
        and official.get("request_level_billing_reconciliation_documented")
        is False
        and official.get("pre_header_attempt_settlement_bound_documented")
        is False,
        "m20_official_contract_invalid",
    )
    require(
        isinstance(decision.get("viable"), str)
        and isinstance(decision.get("not_viable"), str)
        and isinstance(decision.get("next_acquisition"), str),
        "m20_decision_contract_invalid",
    )
    return manifest


def classify_m20_probe(
    returncode: int, stdout: bytes, stderr: bytes
) -> str:
    combined = stdout + b"\n" + stderr
    if (
        returncode == 0
        and M20_SUCCESS_MARKER in stdout
        and M20_FAILURE_MARKER not in stdout
    ):
        return "reachable"
    if (
        b"API key is invalid" in combined
        or b"DeepSeek HTTP 401" in combined
    ):
        return "authentication_rejected"
    if b"DNS resolution failed" in combined:
        return "dns_failed"
    if b"timed out" in combined or b"Timeout" in combined:
        return "response_header_timeout"
    if b"certificate" in combined or b"TLS" in combined:
        return "tls_failed"
    if b"Connection failed" in combined:
        return "connection_failed"
    for status in (400, 402, 403, 408, 422, 429, 500, 503):
        if f"DeepSeek HTTP {status}".encode("ascii") in combined:
            return f"http_{status}"
    return "unknown_failure"


def m20_changed_production_paths(
    starting_revision: str, revision: str
) -> set[str]:
    changed = git_output(
        "diff",
        "--name-only",
        f"{starting_revision}..{revision}",
        "--",
        "crates",
        "Cargo.toml",
        "Cargo.lock",
        "rust-toolchain.toml",
        "config.example.toml",
    )
    return {path for path in changed.splitlines() if path}


def load_m20_admission(
    path: Path,
    manifest: dict[str, Any],
    revision: str,
    binary: Path,
    binary_identity: dict[str, Any],
    output: Path,
) -> dict[str, Any]:
    admission = read_json_object(
        path, "m20_live_admission_unavailable"
    )
    live = admission.get("live_contract", {})
    require(
        admission.get("schema") == M20_ADMISSION_SCHEMA
        and admission.get("candidate_revision") == revision
        and admission.get("candidate_tree")
        == git_output("rev-parse", f"{revision}^{{tree}}")
        and admission.get("candidate_binary") == binary.as_posix()
        and admission.get("candidate_binary_sha256") == file_hash(binary)
        and admission.get("candidate_binary_size_bytes")
        == binary.stat().st_size
        and admission.get("candidate_binary_version")
        == binary_identity["version"]
        and admission.get("contract_manifest_sha256")
        == file_hash(M20_MANIFEST_PATH)
        and admission.get("harness_sha256")
        == file_hash(Path(__file__).resolve())
        and admission.get("official_protocol_revalidated_on")
        == manifest["official_review"]["reviewed_on"]
        and admission.get("official_sources")
        == manifest["official_review"]["sources"]
        and admission.get("offline_gates_passed") is True
        and admission.get("live_api_admitted") is True
        and live.get("output")
        == repository_relative(output, "m20_live_output_scope_invalid")
        and live.get("planned_probes") == 3
        and live.get("maximum_reruns") == 0
        and live.get("stop_on_first_failure") is True
        and live.get("model_requests") == 0
        and live.get("known_api_cost_usd") == 0.0
        and live.get("output_mode")
        == "ignored_0600_exclusive_hash_chained"
        and live.get("credential_path")
        == manifest["formal_viability"]["credential_path"]
        and live.get("credential_contents_read_before_admission") is False
        and live.get("official_api_requests_before_admission") == 0,
        "m20_live_admission_invalid",
    )
    return admission


def m20_preflight(
    binary: Path,
    revision: str,
    admission_path: Path | None,
    output: Path | None,
    *,
    formal: bool,
) -> dict[str, Any]:
    manifest = load_m20_manifest()
    require(
        git_output("branch", "--show-current") == "deepseek-agent",
        "branch_invalid",
    )
    require(not git_output("status", "--porcelain=v1"), "worktree_dirty")
    require(
        git_output("rev-parse", "--verify", f"{revision}^{{commit}}")
        == revision,
        "revision_invalid",
    )
    source = manifest["source_identity"]
    starting_revision = source["starting_revision"]
    require(
        git_output("rev-parse", f"{starting_revision}^{{tree}}")
        == source["starting_tree"],
        "m20_starting_identity_mismatch",
    )
    ancestry = run_command(
        [
            "git",
            "merge-base",
            "--is-ancestor",
            starting_revision,
            revision,
        ],
        cwd=ROOT,
    )
    require(ancestry.returncode == 0, "m20_candidate_ancestry_invalid")
    require(
        m20_changed_production_paths(starting_revision, revision)
        == {
            "crates/deepseek/src/transport.rs",
            "crates/localization/locales/en.json",
            "crates/localization/locales/zh-Hans.json",
            "crates/tui/src/main.rs",
            "crates/tui/tests/qa_pty.rs",
        },
        "m20_production_delta_invalid",
    )
    authority_paths = {
        "product_plan": ROOT / "docs/product/PRODUCT_PLAN.md",
        "roadmap": ROOT / "docs/product/ROADMAP.md",
        "evaluation": ROOT / "docs/product/EVALUATION.md",
        "current_architecture": (
            ROOT / "docs/architecture/CURRENT_CODEWHALE.md"
        ),
    }
    require(
        all(
            file_hash(path)
            == manifest["authority_sha256"][authority]
            for authority, path in authority_paths.items()
        ),
        "m20_authority_identity_mismatch",
    )
    require(
        file_hash(ROOT / "Cargo.lock") == source["cargo_lock_sha256"]
        and file_hash(ROOT / "rust-toolchain.toml")
        == source["rust_toolchain_sha256"],
        "m20_toolchain_identity_mismatch",
    )
    identity = probe_binary(binary, revision)
    admission = None
    if formal:
        require(
            admission_path is not None and output is not None,
            "m20_live_admission_required",
        )
        admission = load_m20_admission(
            admission_path,
            manifest,
            revision,
            binary,
            identity,
            output,
        )
    return {
        "manifest_sha256": file_hash(M20_MANIFEST_PATH),
        "harness_sha256": file_hash(Path(__file__).resolve()),
        "binary": identity,
        "admission_sha256": (
            file_hash(admission_path)
            if admission is not None and admission_path is not None
            else None
        ),
    }


def run_m20_self_test() -> int:
    manifest = load_m20_manifest()
    require(
        classify_m20_probe(
            0,
            b"prefix Official API host and credential are reachable suffix",
            b"",
        )
        == "reachable",
        "m20_success_classification_invalid",
    )
    require(
        classify_m20_probe(
            0,
            b"API connection failed\nAPI key is invalid",
            b"",
        )
        == "authentication_rejected",
        "m20_auth_classification_invalid",
    )
    require(
        classify_m20_probe(
            0,
            b"API connection failed\nDNS resolution failed",
            b"",
        )
        == "dns_failed",
        "m20_dns_classification_invalid",
    )
    require(
        classify_m20_probe(
            0,
            b"Official API host and credential are reachable\nAPI connection failed",
            b"",
        )
        != "reachable",
        "m20_conflicting_marker_accepted",
    )
    with tempfile.TemporaryDirectory(
        prefix="dse-m20-journal-self-test-"
    ) as temporary:
        journal_path = Path(temporary) / "journal.jsonl"
        with Journal.claim(
            journal_path,
            enforce_results_scope=False,
            schema=M20_JOURNAL_SCHEMA,
        ) as journal:
            journal.emit(
                {
                    "record_type": "self_test",
                    "model_requests": 0,
                    "maximum_reruns": 0,
                }
            )
        audit = read_hash_chained_journal(
            journal_path,
            M20_JOURNAL_SCHEMA,
            allow_partial_tail=False,
        )
        require(
            len(audit["records"]) == 1
            and audit["partial_tail_bytes"] == 0,
            "m20_journal_self_test_invalid",
        )
    print(
        json.dumps(
            {
                "schema": M20_MANIFEST_SCHEMA,
                "manifest_sha256": file_hash(M20_MANIFEST_PATH),
                "planned_probes": manifest["formal_viability"][
                    "planned_probes"
                ],
                "model_requests": 0,
                "maximum_reruns": 0,
                "result": "pass",
                "key_accessed": False,
                "network_accessed": False,
            },
            sort_keys=True,
        )
    )
    return 0


def run_m20_dry(args: argparse.Namespace) -> int:
    revision = args.revision or git_output("rev-parse", "HEAD")
    identity = m20_preflight(
        Path(args.binary).resolve(),
        revision,
        None,
        None,
        formal=False,
    )
    print(
        json.dumps(
            {
                "schema": M20_MANIFEST_SCHEMA,
                "record_type": "plan",
                "source_identity": identity,
                "planned_probes": 3,
                "model_requests": 0,
                "known_api_cost_usd": 0.0,
                "maximum_reruns": 0,
                "key_accessed": False,
                "network_accessed": False,
            },
            sort_keys=True,
        )
    )
    return 0


def run_m20_formal(args: argparse.Namespace) -> int:
    require(args.key_file, "key_file_required")
    require(args.output, "output_required")
    require(args.admission, "m20_live_admission_required")
    revision = args.revision or git_output("rev-parse", "HEAD")
    binary = Path(args.binary).resolve()
    output = Path(args.output).resolve()
    identity = m20_preflight(
        binary,
        revision,
        Path(args.admission).resolve(),
        output,
        formal=True,
    )
    with Journal.claim(
        output, schema=M20_JOURNAL_SCHEMA
    ) as journal:
        journal.emit(
            {
                "record_type": "plan",
                "source_identity": identity,
                "planned_probes": 3,
                "model_requests": 0,
                "known_api_cost_usd": 0.0,
                "maximum_reruns": 0,
                "chat_inference_tested": False,
                "pre_header_billing_reconciliation": (
                    "unresolved_by_official_contract"
                ),
            }
        )
        key = read_key(Path(args.key_file).expanduser().resolve())
        journal.emit(
            {
                "record_type": "credential_access",
                "key_accessed": True,
                "network_accessed": False,
            }
        )
        with tempfile.TemporaryDirectory(
            prefix="dse-m20-transport-"
        ) as temporary:
            temporary_root = Path(temporary)
            frozen_binary = temporary_root / "dse-tui"
            workspace = temporary_root / "workspace"
            dse_home = temporary_root / "home"
            shutil.copy2(binary, frozen_binary)
            os.chmod(frozen_binary, 0o500)
            workspace.mkdir(mode=0o700)
            dse_home.mkdir(mode=0o700)
            frozen_identity = probe_binary(frozen_binary, revision)
            require(
                frozen_identity["sha256"]
                == identity["binary"]["sha256"],
                "m20_frozen_binary_mismatch",
            )
            completed = 0
            for ordinal in range(1, 4):
                environment = safe_env()
                environment.update(
                    {
                        "DSE_HOME": str(dse_home),
                        "DEEPSEEK_API_KEY": key,
                        "NO_COLOR": "1",
                        "TERM": "dumb",
                    }
                )
                started = time.monotonic()
                try:
                    result = run_command(
                        [
                            str(frozen_binary),
                            "--language",
                            "en",
                            "--workspace",
                            str(workspace),
                            "--no-project-config",
                            "--skip-onboarding",
                            "doctor",
                        ],
                        cwd=workspace,
                        environment=environment,
                        timeout=30,
                    )
                except EvaluationError as error:
                    journal.emit(
                        {
                            "record_type": "abort",
                            "probe_ordinal": ordinal,
                            "outcome_class": error.code,
                            "completed_probes": completed,
                            "model_requests": 0,
                            "maximum_reruns": 0,
                            "key_accessed": True,
                            "network_accessed": True,
                        }
                    )
                    key = ""
                    return 2
                duration_ms = int(
                    (time.monotonic() - started) * 1000
                )
                key_bytes = key.encode("utf-8")
                require(
                    key_bytes not in result.stdout
                    and key_bytes not in result.stderr,
                    "m20_credential_exposed",
                )
                outcome = classify_m20_probe(
                    result.returncode, result.stdout, result.stderr
                )
                record = {
                    "record_type": "probe_result",
                    "probe_ordinal": ordinal,
                    "outcome_class": outcome,
                    "process_exit": result.returncode,
                    "duration_ms": duration_ms,
                    "stdout_bytes": len(result.stdout),
                    "stdout_sha256": sha256_bytes(result.stdout),
                    "stderr_bytes": len(result.stderr),
                    "stderr_sha256": sha256_bytes(result.stderr),
                    "stdout_or_stderr_retained": False,
                    "credential_or_balance_retained": False,
                    "model_requests": 0,
                    "known_api_cost_usd": 0.0,
                    "maximum_reruns": 0,
                    "key_accessed": True,
                    "network_accessed": True,
                }
                journal.emit(record)
                if outcome != "reachable":
                    journal.emit(
                        {
                            "record_type": "abort",
                            "probe_ordinal": ordinal,
                            "outcome_class": outcome,
                            "completed_probes": completed,
                            "model_requests": 0,
                            "maximum_reruns": 0,
                            "key_accessed": True,
                            "network_accessed": True,
                        }
                    )
                    key = ""
                    return 2
                completed += 1
            journal.emit(
                {
                    "record_type": "summary",
                    "decision": "viable",
                    "completed_probes": completed,
                    "dns_tcp_tls_http_auth": "proven_collectively",
                    "chat_inference_tested": False,
                    "inference_accounting_tested": False,
                    "pre_header_billing_reconciliation": (
                        "unresolved_by_official_contract"
                    ),
                    "model_requests": 0,
                    "known_api_cost_usd": 0.0,
                    "maximum_reruns": 0,
                    "key_accessed": True,
                    "network_accessed": True,
                }
            )
        key = ""
    return 0


def run_dry(args: argparse.Namespace) -> int:
    revision = args.revision or git_output("rev-parse", "HEAD")
    identity = preflight(
        Path(args.binary).resolve(),
        revision,
        None,
        None,
        formal=False,
    )
    print(
        json.dumps(
            plan_record(identity), ensure_ascii=False, sort_keys=True
        )
    )
    return 0


def run_formal(args: argparse.Namespace) -> int:
    require(CAMPAIGN != "m15", "m15_campaign_closed")
    require(args.acknowledge_cost, "cost_acknowledgement_required")
    require(args.key_file, "key_file_required")
    require(args.output, "output_required")
    require(args.admission, "live_admission_required")
    revision = args.revision or git_output("rev-parse", "HEAD")
    binary = Path(args.binary).resolve()
    output = Path(args.output).resolve()
    identity = preflight(
        binary,
        revision,
        Path(args.admission).resolve(),
        output,
        formal=True,
    )
    with Journal.claim(output) as journal:
        journal.emit(plan_record(identity))
        key = read_key(Path(args.key_file).expanduser().resolve())
        journal.emit(
            {
                "record_type": "credential_access",
                "key_accessed": True,
                "network_accessed": False,
            }
        )
        frozen_root = Path(
            tempfile.mkdtemp(prefix=f"codewhale-{CAMPAIGN}-binary-")
        )
        frozen_binary = frozen_root / (
            "dse" if CAMPAIGN in DSE_CAMPAIGNS else "codewhale"
        )
        arms: list[dict[str, Any]] = []
        try:
            shutil.copy2(binary, frozen_binary)
            os.chmod(frozen_binary, 0o500)
            frozen_identity = probe_binary(frozen_binary, revision)
            require(
                frozen_identity["sha256"]
                == identity["binary"]["sha256"],
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
    parser.add_argument(
        "--campaign",
        choices=(
            "m9c",
            "m11",
            "m12",
            "m15",
            "m18",
            "m19",
            "m20b",
            "m23b",
            "m30",
            "m36a2",
        ),
        default="m9c",
    )
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--self-test", action="store_true")
    mode.add_argument("--freeze-report", action="store_true")
    mode.add_argument("--trajectory-report", action="store_true")
    mode.add_argument("--observer-conformance", action="store_true")
    mode.add_argument("--acceptance-conformance", action="store_true")
    mode.add_argument("--interaction-conformance", action="store_true")
    mode.add_argument("--truth-conformance", action="store_true")
    mode.add_argument("--hardness-conformance", action="store_true")
    mode.add_argument(
        "--hardness-continuity-self-test", action="store_true"
    )
    mode.add_argument("--dry-run", action="store_true")
    mode.add_argument("--transport-viability-self-test", action="store_true")
    mode.add_argument("--transport-viability-dry-run", action="store_true")
    mode.add_argument("--transport-viability", action="store_true")
    parser.add_argument("--fault-child")
    parser.add_argument("--self-test-fault", action="store_true")
    parser.add_argument("--binary")
    parser.add_argument("--process-test-binary")
    parser.add_argument("--revision")
    parser.add_argument("--admission")
    parser.add_argument("--acknowledge-cost", action="store_true")
    parser.add_argument("--key-file")
    parser.add_argument("--output")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        require(args.campaign == CAMPAIGN, "campaign_invalid")
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
        if args.trajectory_report:
            return run_trajectory_report()
        if args.observer_conformance:
            return run_observer_conformance()
        if args.acceptance_conformance:
            return run_acceptance_conformance()
        if args.interaction_conformance:
            return run_interaction_conformance()
        if args.truth_conformance:
            return run_truth_conformance()
        if args.hardness_conformance:
            return run_hardness_conformance()
        if args.hardness_continuity_self_test:
            require(args.binary, "binary_required")
            require(
                args.process_test_binary,
                "hardness_process_test_binary_required",
            )
            revision = args.revision or git_output("rev-parse", "HEAD")
            return run_hardness_continuity_self_test(
                Path(args.binary).resolve(),
                Path(args.process_test_binary).resolve(),
                revision,
            )
        if args.transport_viability_self_test:
            return run_m20_self_test()
        require(args.binary, "binary_required")
        if args.transport_viability_dry_run:
            return run_m20_dry(args)
        if args.transport_viability:
            return run_m20_formal(args)
        if args.dry_run:
            return run_dry(args)
        return run_formal(args)
    except EvaluationError as error:
        error_schema = (
            M20_JOURNAL_SCHEMA
            if (
                args.transport_viability_self_test
                or args.transport_viability_dry_run
                or args.transport_viability
            )
            else JOURNAL_SCHEMA
        )
        print(
            json.dumps(
                {
                    "schema": error_schema,
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
