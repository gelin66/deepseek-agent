#!/usr/bin/env python3
"""Project existing canonical evidence into the M7-H request/token waste matrix."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import stat
import tempfile
from collections import defaultdict
from pathlib import Path
from typing import Any, Iterable


SCHEMA = "codewhale.eval.m7-h-request-token-waste.v1"
NUMERIC_FIELDS = (
    "runs",
    "measurement_valid_runs",
    "verified_successes",
    "false_successes",
    "logical_root_requests",
    "physical_root_requests",
    "logical_child_requests",
    "physical_child_requests",
    "input_tokens",
    "output_tokens",
    "reasoning_root_tokens",
    "reasoning_child_tokens",
    "reasoning_unattributed_tokens",
    "reasoning_replay_root_tokens",
    "reasoning_replay_child_tokens",
    "reasoning_replay_unattributed_tokens",
    "compactions",
    "hard_budget_exhausted_runs",
    "handoff_integration_observed_runs",
    "terminal_catalog_unobservable_runs",
)


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_json(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError(f"{path}: top-level JSON must be an object")
    return value


def checked_source(root: Path, source: dict[str, Any]) -> Path:
    path = root / source["path"]
    actual = sha256_file(path)
    expected = source["sha256"]
    if actual != expected:
        raise ValueError(f"{path}: SHA-256 mismatch: {actual} != {expected}")
    return path


def empty_row(dataset: str, group: str, product_metric_eligible: bool) -> dict[str, Any]:
    row: dict[str, Any] = {
        "dataset": dataset,
        "group": group,
        "product_metric_eligible": product_metric_eligible,
    }
    row.update({field: 0 for field in NUMERIC_FIELDS})
    return row


def add_usage(
    row: dict[str, Any],
    usage: dict[str, Any],
    actor: str | None,
) -> None:
    row["input_tokens"] += int(
        usage.get("input_tokens", usage.get("input", 0))
    )
    row["output_tokens"] += int(
        usage.get("output_tokens", usage.get("output", 0))
    )
    reasoning = int(
        usage.get("reasoning_tokens", usage.get("reasoning", 0))
    )
    replay = int(
        usage.get(
            "reasoning_replay_tokens",
            usage.get("reasoning_replay", 0),
        )
    )
    if actor == "root":
        row["reasoning_root_tokens"] += reasoning
        row["reasoning_replay_root_tokens"] += replay
    elif actor == "child":
        row["reasoning_child_tokens"] += reasoning
        row["reasoning_replay_child_tokens"] += replay
    else:
        row["reasoning_unattributed_tokens"] += reasoning
        row["reasoning_replay_unattributed_tokens"] += replay


def project_exec_jsonl(
    path: Path,
    dataset: str,
    product_metric_eligible: bool,
) -> list[dict[str, Any]]:
    grouped: dict[str, dict[str, Any]] = {}
    for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        record = json.loads(line)
        if record.get("record_type") != "run_manifest":
            continue
        group = f"{record['variant']}:{record['lane']}"
        row = grouped.setdefault(
            group, empty_row(dataset, group, product_metric_eligible)
        )
        row["runs"] += 1
        valid = not record.get("measurement_invalid", False)
        row["measurement_valid_runs"] += int(valid)
        row["verified_successes"] += int(record.get("verified_success") is True)
        row["false_successes"] += int(record.get("false_success") is True)

        prompt_evidence = record["production_system_prompt_evidence"]
        logical_root = sum(
            int(run["prepared_request_count"])
            for run in prompt_evidence["runs"]
            if run["actor_kind"] == "root"
        )
        logical_child = sum(
            int(run["prepared_request_count"])
            for run in prompt_evidence["runs"]
            if run["actor_kind"] == "child"
        )
        physical_root = int(record["requests"]["root"]["started"])
        physical_child = int(record["requests"]["child"]["started"])
        if logical_root != physical_root or logical_child != physical_child:
            raise ValueError(
                f"{path}:{line_number}: logical/physical actor request mismatch"
            )
        row["logical_root_requests"] += logical_root
        row["physical_root_requests"] += physical_root
        row["logical_child_requests"] += logical_child
        row["physical_child_requests"] += physical_child
        add_usage(row, record["tokens"], None)
        row["compactions"] += int(prompt_evidence["compaction_attestation_count"])
        row["hard_budget_exhausted_runs"] += int(
            record["requests"].get("budget_exhausted") is True
        )
        row["handoff_integration_observed_runs"] += int(
            record["lane"] == "multi"
            and record.get("lane_contract", {}).get("passed") is True
            and record.get("task_evidence_passed") is True
        )
        # RuntimeEvent v6 persisted prepared request identity but this old
        # evaluator did not retain the per-request advertised catalog.
        row["terminal_catalog_unobservable_runs"] += 1
    return list(grouped.values())


def event_request_count(arm: dict[str, Any], actor: str) -> int:
    return int(arm.get("event_counts", {}).get(actor, {}).get("model_request_prepared", 0))


def project_writer_v2(path: Path) -> list[dict[str, Any]]:
    result = load_json(path)
    grouped: dict[str, dict[str, Any]] = {}
    for pair in result["accepted_pairs"]:
        for arm in pair["arms"]:
            group = str(arm["treatment"])
            row = grouped.setdefault(group, empty_row("m6_b1_writer_v2", group, True))
            row["runs"] += 1
            row["measurement_valid_runs"] += int(arm["measurement_valid"] is True)
            row["verified_successes"] += int(arm["verified_success"] is True)
            row["false_successes"] += int(arm["false_success"] is True)
            logical_root = event_request_count(arm, "root")
            logical_child = event_request_count(arm, "child")
            physical_root = int(arm["requests"]["root"]["started"])
            physical_child = int(arm["requests"]["child"]["started"])
            if logical_root != physical_root or logical_child != physical_child:
                raise ValueError(f"{path}: Writer logical/physical actor request mismatch")
            row["logical_root_requests"] += logical_root
            row["physical_root_requests"] += physical_root
            row["logical_child_requests"] += logical_child
            row["physical_child_requests"] += physical_child
            add_usage(row, arm["actor_usage"]["root"]["usage"], "root")
            add_usage(row, arm["actor_usage"]["child"]["usage"], "child")
            row["compactions"] += int(
                arm.get("event_counts", {})
                .get("root", {})
                .get("context_compaction_committed", 0)
            ) + int(
                arm.get("event_counts", {})
                .get("child", {})
                .get("context_compaction_committed", 0)
            )
            row["hard_budget_exhausted_runs"] += int(
                arm["requests"].get("budget_exhausted") is True
            )
            row["handoff_integration_observed_runs"] += int(
                group == "writer" and arm.get("writer", {}).get("successful_chain") is True
            )
            row["terminal_catalog_unobservable_runs"] += 1
    return list(grouped.values())


def project_m7_a2(path: Path) -> list[dict[str, Any]]:
    result = load_json(path)
    grouped: dict[str, dict[str, Any]] = {}
    for arm in result["arms"]:
        group = str(arm["variant"])
        row = grouped.setdefault(group, empty_row("m7_a2_partial", group, False))
        row["runs"] += 1
        row["measurement_valid_runs"] += int(arm["measurement_valid"] is True)
        row["verified_successes"] += int(arm["verified_success"] is True)
        row["false_successes"] += int(arm["false_success"] is True)
        logical_root = int(arm["event_counts"].get("model_request_prepared", 0))
        logical_child = int(arm["child"].get("logical_model_requests", 0))
        physical_root = int(arm["accounting"]["root"]["started"])
        physical_child = int(arm["accounting"]["child"]["started"])
        if logical_root != physical_root or logical_child != physical_child:
            raise ValueError(f"{path}: M7-A2 logical/physical actor request mismatch")
        row["logical_root_requests"] += logical_root
        row["physical_root_requests"] += physical_root
        row["logical_child_requests"] += logical_child
        row["physical_child_requests"] += physical_child
        actor = "root" if physical_child == 0 else None
        add_usage(row, arm["accounting"]["usage"], actor)
        row["compactions"] += int(
            arm["event_counts"].get("context_compaction_committed", 0)
        )
        row["terminal_catalog_unobservable_runs"] += 1
    return list(grouped.values())


def dataset_totals(rows: Iterable[dict[str, Any]]) -> list[dict[str, Any]]:
    grouped: dict[str, dict[str, int]] = defaultdict(
        lambda: {field: 0 for field in NUMERIC_FIELDS}
    )
    for row in rows:
        totals = grouped[row["dataset"]]
        for field in NUMERIC_FIELDS:
            totals[field] += int(row[field])
    return [
        {"dataset": dataset, **totals}
        for dataset, totals in sorted(grouped.items())
    ]


def observe(root: Path, manifest_path: Path) -> dict[str, Any]:
    manifest_bytes = manifest_path.read_bytes()
    manifest = json.loads(manifest_bytes)
    if manifest.get("schema") != SCHEMA:
        raise ValueError("unexpected manifest schema")
    sources = {source["id"]: source for source in manifest["sources"]}
    terminal = checked_source(root, sources["terminal_permit_v1"])
    eager = checked_source(root, sources["eager_join_v1"])
    writer = checked_source(root, sources["writer_v2"])
    m7_a2 = checked_source(root, sources["m7_a2_partial"])

    rows = []
    rows.extend(project_exec_jsonl(terminal, "terminal_permit_v1", True))
    rows.extend(project_exec_jsonl(eager, "eager_join_v1", True))
    rows.extend(project_writer_v2(writer))
    rows.extend(project_m7_a2(m7_a2))
    rows.sort(key=lambda row: (row["dataset"], row["group"]))

    mismatches = sum(
        abs(row["logical_root_requests"] - row["physical_root_requests"])
        + abs(row["logical_child_requests"] - row["physical_child_requests"])
        for row in rows
    )
    return {
        "schema": SCHEMA,
        "record_type": "offline_observation",
        "suite_id": manifest["suite_id"],
        "manifest_sha256": sha256_bytes(manifest_bytes),
        "source_sha256": {
            source_id: source["sha256"] for source_id, source in sorted(sources.items())
        },
        "matrix": rows,
        "dataset_totals": dataset_totals(rows),
        "cross_checks": {
            "logical_physical_request_mismatch_total": mismatches,
            "all_source_hashes_match": True,
            "credential_read": False,
            "network_accessed": False,
            "maximum_reruns": manifest["maximum_reruns"],
        },
        "counterexample": manifest["counterexample"],
        "interpretation": {
            "historical_rows_are_overlapping_claims": True,
            "terminal_catalog_identity_missing_from_legacy_raw": True,
            "writer_actor_usage_is_exact": True,
            "m7_a2_product_metric_eligible": False,
            "live_api_admitted": False,
            "reason": "deterministic Host admission correctness is proven offline; no model treatment or paid efficiency claim is made",
        },
    }


def write_exclusive(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    descriptor = os.open(path, flags, 0o600)
    try:
        payload = json.dumps(
            value, ensure_ascii=False, sort_keys=True, separators=(",", ":")
        ).encode("utf-8")
        os.write(descriptor, payload)
        os.write(descriptor, b"\n")
        os.fsync(descriptor)
    finally:
        os.close(descriptor)
    mode = stat.S_IMODE(path.stat().st_mode)
    if mode != 0o600:
        raise ValueError(f"{path}: expected mode 0600, got {mode:04o}")


def self_test() -> None:
    with tempfile.TemporaryDirectory(prefix="codewhale-m7h-self-test-") as directory:
        path = Path(directory) / "result.json"
        value = {"schema": SCHEMA, "ok": True}
        write_exclusive(path, value)
        assert stat.S_IMODE(path.stat().st_mode) == 0o600
        assert json.loads(path.read_text(encoding="utf-8")) == value
        try:
            write_exclusive(path, value)
        except FileExistsError:
            pass
        else:
            raise AssertionError("existing output must fail closed")
        row = empty_row("fixture", "root", False)
        add_usage(
            row,
            {
                "input_tokens": 11,
                "output_tokens": 3,
                "reasoning_tokens": 2,
                "reasoning_replay_tokens": 5,
            },
            "root",
        )
        assert row["input_tokens"] == 11
        assert row["reasoning_root_tokens"] == 2
        assert row["reasoning_replay_root_tokens"] == 5
        legacy = empty_row("fixture", "legacy", False)
        add_usage(
            legacy,
            {
                "input": 13,
                "output": 4,
                "reasoning": 3,
                "reasoning_replay": 7,
            },
            None,
        )
        assert legacy["input_tokens"] == 13
        assert legacy["reasoning_unattributed_tokens"] == 3
        assert legacy["reasoning_replay_unattributed_tokens"] == 7


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--self-test", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.self_test:
        self_test()
        print("m7-h request/token observer self-test: pass")
        return 0
    if args.manifest is None or args.output is None:
        raise SystemExit("--manifest and --output are required")
    root = Path(__file__).resolve().parents[1]
    result = observe(root, args.manifest.resolve())
    write_exclusive(args.output.resolve(), result)
    print(
        json.dumps(
            {
                "status": "passed",
                "suite_id": result["suite_id"],
                "rows": len(result["matrix"]),
                "output": str(args.output.resolve()),
                "credential_read": False,
                "network_accessed": False,
            },
            ensure_ascii=False,
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
