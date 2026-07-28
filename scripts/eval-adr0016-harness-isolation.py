#!/usr/bin/env python3
"""Validate ADR-0016's offline continuation and Harness-isolation baseline.

This evaluator never calls DeepSeek and does not implement a second Agent loop.
It validates frozen trajectory facts, the explicitly bounded minimal-loop
contract, and exact current Rust conformance owners. Optional gates execute only
those named offline tests with Cargo networking disabled.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "eval/manifests/adr0016-harness-isolation-offline-v1.json"
EXPECTED_SCHEMA = "dse.eval.adr0016-harness-isolation-offline.v1"
EXPECTED_CAPABILITY_IDS = [f"C{index:02d}" for index in range(1, 8)]
TARGET_DIR = "/private/tmp/dse-adr0016-harness-isolation-target"


class BaselineError(RuntimeError):
    """The frozen baseline is incomplete or contradicts its source evidence."""


def require(condition: bool, code: str) -> None:
    if not condition:
        raise BaselineError(code)


def load_json(path: Path) -> dict:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise BaselineError(f"invalid_json:{path.relative_to(ROOT)}:{error}") from error
    require(isinstance(value, dict), f"json_root_must_be_object:{path.relative_to(ROOT)}")
    return value


def repository_file(relative: str) -> Path:
    path = (ROOT / relative).resolve()
    try:
        path.relative_to(ROOT)
    except ValueError as error:
        raise BaselineError(f"source_path_escapes_repository:{relative}") from error
    require(path.is_file(), f"source_missing:{relative}")
    return path


def require_text(path: Path, needles: list[str]) -> None:
    try:
        text = path.read_text(encoding="utf-8")
    except (OSError, UnicodeError) as error:
        raise BaselineError(f"source_unreadable:{path.relative_to(ROOT)}:{error}") from error
    for needle in needles:
        require(isinstance(needle, str) and bool(needle), "required_text_must_be_nonempty")
        require(needle in text, f"source_fact_missing:{path.relative_to(ROOT)}:{needle}")


def validate_slice_contract(manifest: dict) -> None:
    contract = manifest.get("slice_contract")
    require(isinstance(contract, dict), "slice_contract_missing")
    required = {
        "problem",
        "acceptance",
        "owner",
        "replaces",
        "evidence",
        "cutover_deletion",
    }
    require(required.issubset(contract), "slice_contract_incomplete")
    require(all(isinstance(contract[key], str) and contract[key] for key in required),
            "slice_contract_values_must_be_nonempty")


def validate_comparison_identity(manifest: dict) -> dict:
    identity = manifest.get("comparison_identity")
    require(isinstance(identity, dict), "comparison_identity_missing")
    require(identity.get("class") == "harness_comparison_same_deepseek_model",
            "comparison_class_mismatch")
    require(identity.get("model") == "deepseek-v4-pro", "model_mismatch")
    require(identity.get("reasoning_effort") == "high", "reasoning_effort_mismatch")
    require(identity.get("surface") == "Standard streaming ChatCompletions",
            "surface_mismatch")
    require(identity.get("official_requests") == 0, "official_requests_must_be_zero")
    require(identity.get("maximum_reruns") == 0, "maximum_reruns_must_be_zero")
    require(identity.get("credential_read") is False, "credential_read_must_be_false")
    require(identity.get("product_metric_eligible") is False,
            "offline_baseline_must_not_be_product_metric")
    require(identity.get("quality_comparison_executed") is False,
            "offline_baseline_must_not_claim_quality_comparison")
    held = identity.get("held_constant_by_contract")
    require(isinstance(held, list) and len(held) == len(set(held)),
            "held_constant_contract_invalid")
    require(set(held) == {"model", "reasoning_effort", "surface", "task", "budget", "workspace"},
            "held_constant_contract_incomplete")
    return identity


def validate_continuation_audit(manifest: dict, identity: dict) -> dict:
    audit = manifest.get("continuation_audit")
    require(isinstance(audit, dict), "continuation_audit_missing")
    threshold = audit.get("minimum_independent_same_loss_tasks")
    observed = audit.get("observed_independent_same_loss_tasks")
    require(threshold == 2, "continuation_threshold_must_be_two")
    require(observed == 0, "current_continuation_loss_count_must_be_zero")
    require(audit.get("admissible_current_continuation_losses") == [],
            "admissible_current_continuation_losses_must_be_empty")
    require(audit.get("decision") == "reject_verified_milestone_no_repeated_loss",
            "continuation_decision_mismatch")

    evidence = audit.get("evidence")
    require(isinstance(evidence, list) and len(evidence) == 3,
            "continuation_evidence_must_have_three_sources")
    evidence_ids = [item.get("id") for item in evidence if isinstance(item, dict)]
    require(len(evidence_ids) == 3 and len(set(evidence_ids)) == 3,
            "continuation_evidence_ids_must_be_unique")
    for item in evidence:
        require(isinstance(item, dict), "continuation_evidence_must_be_object")
        path = repository_file(item.get("path", ""))
        needles = item.get("required_text")
        require(isinstance(needles, list) and needles, "continuation_required_text_missing")
        require_text(path, needles)

    task_source = audit.get("task_identity_source")
    require(isinstance(task_source, dict), "task_identity_source_missing")
    source = load_json(repository_file(task_source.get("path", "")))
    resources = source.get("resources", {})
    continuity = source.get("continuity_policy", {})
    require(resources.get("model") == identity["model"], "m36_model_identity_mismatch")
    require(resources.get("reasoning_effort") == identity["reasoning_effort"],
            "m36_reasoning_identity_mismatch")
    require(continuity.get("restarts_per_arm") == task_source.get("required_restarts_per_arm"),
            "m36_restart_contract_mismatch")
    require(continuity.get("physical_requests_added_at_reopen")
            == task_source.get("required_physical_requests_added_at_reopen"),
            "m36_reopen_request_contract_mismatch")
    task_ids = task_source.get("task_ids")
    require(isinstance(task_ids, list) and len(task_ids) == 3 and len(set(task_ids)) == 3,
            "long_horizon_task_ids_must_be_three_unique_ids")
    overlays = source.get("task_overlays", {})
    schedule = source.get("formal_schedule", {}).get("round_order", [])
    scheduled = {task for round_ids in schedule for task in round_ids}
    require(all(task_id in overlays and task_id in scheduled for task_id in task_ids),
            "long_horizon_task_identity_missing")
    return {
        "threshold": threshold,
        "observed": observed,
        "admissible_loss_codes": [],
        "long_horizon_verified": 3,
        "long_horizon_total": 3,
        "decision": audit["decision"],
    }


def validate_concepts(manifest: dict) -> dict:
    minimal = manifest.get("minimal_harness_contract")
    current = manifest.get("dse_current_contract")
    require(isinstance(minimal, dict), "minimal_harness_contract_missing")
    require(isinstance(current, dict), "dse_current_contract_missing")
    minimal_concepts = minimal.get("comparison_concepts")
    base_concepts = current.get("base_comparison_concepts")
    additional = current.get("additional_comparison_concepts")
    require(isinstance(minimal_concepts, list) and len(minimal_concepts) == 4,
            "minimal_concept_count_must_be_four")
    require(len(minimal_concepts) == len(set(minimal_concepts)),
            "minimal_concepts_must_be_unique")
    require(base_concepts == minimal_concepts, "dse_base_must_match_minimal_contract")
    require(isinstance(additional, list) and len(additional) == 6,
            "dse_additional_concept_count_must_be_six")
    require(len(additional) == len(set(additional)), "dse_additional_concepts_must_be_unique")
    require(not set(minimal_concepts).intersection(additional), "concept_inventories_must_not_overlap")
    require(minimal.get("durable_store") is False, "minimal_contract_must_not_have_store")
    require(minimal.get("host_terminal_authority") is False,
            "minimal_contract_must_not_have_host_terminal_authority")
    require(minimal.get("actor_scoped_authorization") is False,
            "minimal_contract_must_not_have_actor_authorization")
    require(minimal.get("writer_worktree_lifecycle") is False,
            "minimal_contract_must_not_have_writer_lifecycle")
    return {
        "minimal_comparison_concepts": len(minimal_concepts),
        "dse_current_comparison_concepts": len(minimal_concepts) + len(additional),
        "dse_additional_concepts": additional,
    }


def validate_capabilities(manifest: dict) -> tuple[dict, list[dict]]:
    matrix = manifest.get("capability_matrix")
    require(isinstance(matrix, list), "capability_matrix_missing")
    require([item.get("id") for item in matrix if isinstance(item, dict)]
            == EXPECTED_CAPABILITY_IDS, "capability_ids_must_be_exact_C01_through_C07")
    gate_specs: list[dict] = []
    for item in matrix:
        require(isinstance(item, dict), "capability_entry_must_be_object")
        require(item.get("dse_current_capable") is True, "dse_capability_must_be_true")
        require(isinstance(item.get("minimal_contract_capable"), bool),
                "minimal_capability_must_be_boolean")
        evidence = item.get("evidence")
        require(isinstance(evidence, dict), "capability_evidence_missing")
        source_path = repository_file(evidence.get("path", ""))
        if "filter" in evidence:
            test_filter = evidence.get("filter")
            package = evidence.get("package")
            require(isinstance(test_filter, str) and bool(test_filter), "test_filter_missing")
            require(isinstance(package, str) and bool(package), "test_package_missing")
            source = source_path.read_text(encoding="utf-8")
            pattern = rf"(?:async\s+)?fn\s+{re.escape(test_filter)}\s*\("
            require(re.search(pattern, source) is not None,
                    f"exact_test_owner_missing:{source_path.relative_to(ROOT)}:{test_filter}")
            gate_specs.append({"id": item["id"], "package": package, "filter": test_filter})
        else:
            required = evidence.get("required_text")
            require(isinstance(required, str) and bool(required), "capability_required_text_missing")
            require_text(source_path, [required])
    minimal_count = sum(item["minimal_contract_capable"] for item in matrix)
    current_count = sum(item["dse_current_capable"] for item in matrix)
    require(minimal_count == 1, "minimal_capability_count_must_be_one")
    require(current_count == 7, "dse_capability_count_must_be_seven")
    return {
        "capabilities_total": len(matrix),
        "minimal_contract_capabilities": minimal_count,
        "dse_current_capabilities": current_count,
    }, gate_specs


def validate_expected_result(manifest: dict, continuation: dict, concepts: dict,
                             capabilities: dict) -> dict:
    expected = manifest.get("expected_result")
    require(isinstance(expected, dict), "expected_result_missing")
    actual = {
        "continuation_loss_threshold_met": continuation["observed"] >= continuation["threshold"],
        "verified_milestone_projection": "reject_not_implemented",
        "minimal_contract_capabilities": capabilities["minimal_contract_capabilities"],
        "dse_current_capabilities": capabilities["dse_current_capabilities"],
        "minimal_comparison_concepts": concepts["minimal_comparison_concepts"],
        "dse_current_comparison_concepts": concepts["dse_current_comparison_concepts"],
        "decision": "keep_current_harness_reject_verified_milestone_no_repeated_loss",
        "production_rust_delta": 0,
        "official_requests": 0,
    }
    require(expected == actual, "expected_result_does_not_match_derived_result")
    return actual


def run_gates(gates: list[dict]) -> list[dict]:
    environment = os.environ.copy()
    environment["CARGO_INCREMENTAL"] = "0"
    environment["CARGO_NET_OFFLINE"] = "true"
    environment["CARGO_TARGET_DIR"] = TARGET_DIR
    for key in list(environment):
        if key.startswith("DEEPSEEK_"):
            environment.pop(key)
    results = []
    for gate in gates:
        command = [
            "cargo",
            "test",
            "-p",
            gate["package"],
            "--locked",
            gate["filter"],
        ]
        completed = subprocess.run(
            command,
            cwd=ROOT,
            env=environment,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
        )
        result = {**gate, "exit_code": completed.returncode}
        if completed.returncode != 0:
            result["diagnostic_tail"] = completed.stdout[-65_536:]
        results.append(result)
        if completed.returncode != 0:
            break
    return results


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--run-gates",
        action="store_true",
        help="run the six exact offline Rust conformance filters",
    )
    parser.add_argument("--pretty", action="store_true", help="pretty-print the result JSON")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        raw = MANIFEST.read_bytes()
        manifest = json.loads(raw)
        require(isinstance(manifest, dict), "manifest_root_must_be_object")
        require(manifest.get("schema") == EXPECTED_SCHEMA, "manifest_schema_mismatch")
        validate_slice_contract(manifest)
        identity = validate_comparison_identity(manifest)
        continuation = validate_continuation_audit(manifest, identity)
        concepts = validate_concepts(manifest)
        capabilities, gates = validate_capabilities(manifest)
        decision = validate_expected_result(manifest, continuation, concepts, capabilities)
        gate_results = run_gates(gates) if args.run_gates else []
        gate_passed = all(item["exit_code"] == 0 for item in gate_results)
        if args.run_gates:
            require(len(gate_results) == len(gates) and gate_passed, "offline_gate_failed")
        result = {
            "schema": "dse.eval.adr0016-harness-isolation-offline-result.v1",
            "status": "pass",
            "manifest_sha256": hashlib.sha256(raw).hexdigest(),
            "comparison_class": identity["class"],
            "model": identity["model"],
            "reasoning_effort": identity["reasoning_effort"],
            "continuation_audit": continuation,
            "capability_baseline": capabilities,
            "complexity_baseline": concepts,
            "decision": decision,
            "offline_gates": {
                "requested": args.run_gates,
                "passed": len(gate_results) if gate_passed else 0,
                "total": len(gates),
                "results": gate_results,
            },
            "credential_read": False,
            "official_requests": 0,
            "product_metric_eligible": False,
            "quality_comparison_executed": False,
        }
    except (OSError, UnicodeError, json.JSONDecodeError, BaselineError) as error:
        print(f"adr0016_harness_isolation_error:{error}", file=sys.stderr)
        return 1
    print(json.dumps(result, ensure_ascii=False, sort_keys=True,
                     indent=2 if args.pretty else None,
                     separators=None if args.pretty else (",", ":")))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
