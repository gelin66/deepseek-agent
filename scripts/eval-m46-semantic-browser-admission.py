#!/usr/bin/env python3
"""Evaluate the M46 read-only semantic-browser admission gate.

The evaluator is credential-free and loopback-only. It invokes the current
ProductionToolExecutor through one test-only Rust caller, then uses Playwright
solely as an external DOM/accessibility oracle. It does not implement or
install a production browser path.
"""

from __future__ import annotations

import argparse
import copy
import hashlib
import http.client
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import tempfile
import time
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
MANIFEST_PATH = ROOT / "eval/manifests/m46-semantic-browser-admission-v1.json"
EXPECTED_SCHEMA = "dse.eval.m46-semantic-browser-admission.v1"
RESULT_SCHEMA = "dse.eval.m46-semantic-browser-admission-result.v1"
CONTROL_MARKER = "M46_CONTROL_MATRIX="
CONTROL_TEST = "m46_current_controls_cannot_claim_js_only_rendered_application_state"


class AdmissionError(RuntimeError):
    """The pre-registered identity, evidence, or decision is invalid."""


def require(condition: bool, code: str) -> None:
    if not condition:
        raise AdmissionError(code)


def load_manifest() -> dict[str, Any]:
    try:
        value = json.loads(MANIFEST_PATH.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise AdmissionError(f"manifest_unreadable:{error}") from error
    require(isinstance(value, dict), "manifest_root_must_be_object")
    return value


def repository_file(relative: str) -> Path:
    require(isinstance(relative, str) and bool(relative), "repository_path_missing")
    path = (ROOT / relative).resolve()
    try:
        path.relative_to(ROOT)
    except ValueError as error:
        raise AdmissionError(f"repository_path_escape:{relative}") from error
    require(path.is_file(), f"repository_file_missing:{relative}")
    return path


def file_sha256(path: Path) -> str:
    return "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()


def validate_manifest(manifest: dict[str, Any]) -> list[dict[str, Any]]:
    require(manifest.get("schema") == EXPECTED_SCHEMA, "manifest_schema_mismatch")
    baseline = manifest.get("baseline_commit")
    require(
        baseline == "997c67e20eb67b663eaf424e334b3ee80e914bee",
        "baseline_commit_mismatch",
    )

    contract = manifest.get("slice_contract")
    require(isinstance(contract, dict), "slice_contract_missing")
    for field in (
        "problem",
        "acceptance",
        "owner",
        "replaces",
        "evidence",
        "cutover_deletion",
    ):
        require(
            isinstance(contract.get(field), str) and bool(contract[field]),
            f"slice_contract_field_missing:{field}",
        )

    identity = manifest.get("identity")
    require(isinstance(identity, dict), "identity_missing")
    require(
        identity.get("control_surface")
        == ["current_production_web_fetch", "current_host_only_application_probe"],
        "control_surface_mismatch",
    )
    require(identity.get("current_model_catalog_size") == 13, "catalog_size_mismatch")
    require(identity.get("model_visible_browser_tools") == 0, "browser_tool_must_be_absent")
    require(identity.get("official_deepseek_requests") == 0, "official_requests_must_be_zero")
    require(identity.get("credential_read") is False, "credential_read_must_be_false")
    require(identity.get("external_network_allowed") is False, "external_network_must_be_false")
    require(identity.get("maximum_reruns") == 0, "maximum_reruns_must_be_zero")
    require(identity.get("actual_cost_usd") == 0, "actual_cost_must_be_zero")
    require(identity.get("product_metric_eligible") is False, "product_metric_must_be_false")
    require(identity.get("quality_comparison_executed") is False, "quality_comparison_must_be_false")

    rule = manifest.get("admission_rule")
    require(isinstance(rule, dict), "admission_rule_missing")
    require(rule.get("minimum_independent_same_loss_tasks") == 2, "loss_threshold_must_be_two")
    require(rule.get("mixed_loss_codes_may_not_be_combined") is True, "mixed_loss_rule_missing")
    require(rule.get("oracle_must_pass") is True, "oracle_gate_missing")
    require(rule.get("control_false_success_must_equal") == 0, "false_success_gate_mismatch")
    allowed_losses = rule.get("allowed_loss_codes")
    require(
        allowed_losses == ["tools:javascript_rendering", "tools:application_visibility"],
        "allowed_loss_codes_mismatch",
    )

    ground_truth = manifest.get("ground_truth")
    require(isinstance(ground_truth, dict), "ground_truth_missing")
    require(
        ground_truth.get("kind") == "eval_only_playwright_dom_accessibility_oracle",
        "oracle_kind_mismatch",
    )
    require(ground_truth.get("screenshots") is False, "screenshots_must_be_false")
    require(ground_truth.get("production_dependency") is False, "oracle_must_not_be_production")
    require(ground_truth.get("max_observation_nodes") == 1, "observation_node_bound_mismatch")
    max_bytes = ground_truth.get("max_observation_bytes")
    require(isinstance(max_bytes, int) and 0 < max_bytes <= 4096, "observation_byte_bound_invalid")
    repository_file(ground_truth.get("runner", ""))
    repository_file(ground_truth.get("node_runner", ""))

    shared = manifest.get("shared_fixture")
    require(isinstance(shared, dict), "shared_fixture_missing")
    server = repository_file(shared.get("server_path", ""))
    require(file_sha256(server) == shared.get("server_sha256"), "server_sha256_mismatch")

    tasks = manifest.get("tasks")
    require(isinstance(tasks, list) and len(tasks) == 2, "task_count_must_be_two")
    task_ids: set[str] = set()
    independence_keys: set[str] = set()
    for task in tasks:
        require(isinstance(task, dict), "task_must_be_object")
        task_id = task.get("task_id")
        independence_key = task.get("independence_key")
        require(isinstance(task_id, str) and bool(task_id), "task_id_missing")
        require(task_id not in task_ids, f"duplicate_task_id:{task_id}")
        task_ids.add(task_id)
        require(
            isinstance(independence_key, str) and independence_key not in independence_keys,
            f"independence_key_invalid:{task_id}",
        )
        independence_keys.add(independence_key)
        require(task.get("loss_code") in allowed_losses, f"loss_code_not_allowed:{task_id}")
        require(
            isinstance(task.get("task_contract"), str) and bool(task["task_contract"]),
            f"task_contract_missing:{task_id}",
        )
        fixture = repository_file(task.get("fixture_path", ""))
        require(file_sha256(fixture) == task.get("fixture_sha256"), f"fixture_sha256_mismatch:{task_id}")
        source = fixture.read_text(encoding="utf-8")
        expected = task.get("expected_rendered_observation")
        require(isinstance(expected, dict), f"expected_observation_missing:{task_id}")
        for field in ("title", "role", "accessible_name", "state_attribute", "state_value"):
            require(
                isinstance(expected.get(field), str) and bool(expected[field]),
                f"expected_field_missing:{task_id}:{field}",
            )
        require("<script>" in source and "</script>" in source, f"js_fixture_missing:{task_id}")
        require(
            source.count("{{DSE_APPLICATION_PROBE_LEASE}}") == 1,
            f"lease_placeholder_mismatch:{task_id}",
        )
        require(
            expected["accessible_name"] not in source,
            f"rendered_name_present_in_raw_http:{task_id}",
        )
        control = task.get("expected_control")
        require(isinstance(control, dict), f"expected_control_missing:{task_id}")
        require(
            control.get("web_fetch") == "rendered_accessible_name_absent_after_script_suppression",
            f"web_fetch_control_mismatch:{task_id}",
        )
        require(
            control.get("application_probe_failure_code") == "application_probe_body_mismatch",
            f"probe_control_mismatch:{task_id}",
        )
        require(control.get("application_probe_health_ready") is True, f"probe_health_gate_missing:{task_id}")
        require(control.get("application_probe_teardown_settled") is True, f"probe_teardown_gate_missing:{task_id}")

    next_contract = manifest.get("next_w2_contract_if_admitted")
    require(isinstance(next_contract, dict), "next_w2_contract_missing")
    require(next_contract.get("owner") == "crates/tools", "next_w2_owner_mismatch")
    require(
        next_contract.get("scope")
        == ["browser_navigate", "bounded_dom_accessibility_snapshot", "host_owned_teardown"],
        "next_w2_scope_mismatch",
    )
    require(
        next_contract.get("production_implementation_in_this_audit") is False,
        "production_browser_must_not_start",
    )
    forbidden = next_contract.get("forbidden")
    require(isinstance(forbidden, list) and len(forbidden) == len(set(forbidden)), "forbidden_scope_invalid")
    return tasks


def safe_base_environment() -> dict[str, str]:
    allowed = (
        "PATH",
        "HOME",
        "USER",
        "TMPDIR",
        "LANG",
        "LC_ALL",
        "RUSTUP_HOME",
        "CARGO_HOME",
        "RUSTC",
        "RUSTDOC",
    )
    return {name: os.environ[name] for name in allowed if name in os.environ}


def run_control_matrix(tasks: list[dict[str, Any]]) -> list[dict[str, Any]]:
    environment = safe_base_environment()
    environment["CARGO_NET_OFFLINE"] = "true"
    environment["CARGO_INCREMENTAL"] = "0"
    command = [
        "cargo",
        "test",
        "-p",
        "dse-tools",
        "--test",
        "m46_semantic_browser_admission",
        "--locked",
        CONTROL_TEST,
        "--",
        "--exact",
        "--nocapture",
    ]
    completed = subprocess.run(
        command,
        cwd=ROOT,
        env=environment,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        timeout=600,
        check=False,
    )
    require(completed.returncode == 0, f"production_control_failed:{completed.stdout[-4000:]}")
    marker_lines = [line for line in completed.stdout.splitlines() if CONTROL_MARKER in line]
    require(len(marker_lines) == 1, "production_control_marker_missing")
    encoded = marker_lines[0].split(CONTROL_MARKER, 1)[1]
    try:
        matrix = json.loads(encoded)
    except json.JSONDecodeError as error:
        raise AdmissionError(f"production_control_json_invalid:{error}") from error
    require(isinstance(matrix, list) and len(matrix) == len(tasks), "production_control_count_mismatch")
    expected_ids = [task["task_id"] for task in tasks]
    require([row.get("task_id") for row in matrix] == expected_ids, "production_control_task_order_mismatch")
    for row, task in zip(matrix, tasks, strict=True):
        require(row.get("loss_code") == task["loss_code"], f"production_control_loss_mismatch:{row.get('task_id')}")
        require(row.get("web_fetch_observed_rendered_state") is False, f"web_fetch_false_success:{row.get('task_id')}")
        require(row.get("application_probe_failure_code") == "application_probe_body_mismatch", f"probe_failure_mismatch:{row.get('task_id')}")
        require(row.get("application_probe_health_ready") is True, f"probe_not_healthy:{row.get('task_id')}")
        require(row.get("application_probe_teardown_settled") is True, f"probe_teardown_not_settled:{row.get('task_id')}")
        require(row.get("application_probe_verdict") == "failed", f"probe_verdict_mismatch:{row.get('task_id')}")
        require(row.get("latest_revision_bound") is True, f"latest_revision_not_bound:{row.get('task_id')}")
        require(row.get("control_false_success") is False, f"control_false_success:{row.get('task_id')}")
    return matrix


def reserve_loopback_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.bind(("127.0.0.1", 0))
        return int(listener.getsockname()[1])


def wait_for_health(port: int, process: subprocess.Popen[bytes]) -> None:
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        require(process.poll() is None, "oracle_server_exited_early")
        try:
            connection = http.client.HTTPConnection("127.0.0.1", port, timeout=0.2)
            connection.request("GET", "/health")
            response = connection.getresponse()
            response.read()
            connection.close()
            if response.status == 200:
                return
        except OSError:
            time.sleep(0.05)
    raise AdmissionError("oracle_server_health_timeout")


def stop_process_group(process: subprocess.Popen[bytes]) -> None:
    if process.poll() is None:
        os.killpg(process.pid, signal.SIGTERM)
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait(timeout=5)
    require(process.poll() is not None, "oracle_server_not_reaped")


def run_oracle(task: dict[str, Any], manifest: dict[str, Any]) -> dict[str, Any]:
    ground_truth = manifest["ground_truth"]
    server_path = repository_file(manifest["shared_fixture"]["server_path"])
    fixture_path = repository_file(task["fixture_path"])
    require(fixture_path.parent == server_path.parent, "fixture_server_root_mismatch")
    port = reserve_loopback_port()
    origin = f"http://127.0.0.1:{port}"
    server_environment = {
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "HOST": "127.0.0.1",
        "PORT": str(port),
        "M46_FIXTURE_HTML": fixture_path.name,
    }
    process = subprocess.Popen(
        [
            "/usr/bin/python3",
            "-I",
            "-B",
            str(server_path),
            f"dse-application-probe:m46-eval-{task['task_id']}",
        ],
        cwd=server_path.parent,
        env=server_environment,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        start_new_session=True,
    )
    try:
        wait_for_health(port, process)
        with tempfile.TemporaryDirectory(prefix="dse-m46-oracle-home-") as oracle_home:
            oracle_environment = {
                "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
                "HOME": oracle_home,
                "NODE_PATH": ground_truth["playwright_node_path"],
                "M46_TASK_ID": task["task_id"],
                "M46_TARGET_URL": origin + "/",
                "M46_ALLOWED_ORIGIN": origin,
                "M46_CHROME_PATH": ground_truth["chrome_path"],
                "M46_EXPECTED_OBSERVATION": json.dumps(
                    task["expected_rendered_observation"],
                    sort_keys=True,
                    separators=(",", ":"),
                ),
                "M46_MAX_OBSERVATION_BYTES": str(ground_truth["max_observation_bytes"]),
            }
            completed = subprocess.run(
                ["node", str(repository_file(ground_truth["node_runner"]))],
                cwd=ROOT,
                env=oracle_environment,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                timeout=30,
                check=False,
            )
        require(completed.returncode == 0, f"oracle_failed:{task['task_id']}:{completed.stderr.strip()}")
        try:
            observation = json.loads(completed.stdout)
        except json.JSONDecodeError as error:
            raise AdmissionError(f"oracle_json_invalid:{task['task_id']}:{error}") from error
    finally:
        stop_process_group(process)

    expected = task["expected_rendered_observation"]
    require(observation.get("task_id") == task["task_id"], f"oracle_task_mismatch:{task['task_id']}")
    require(observation.get("title") == expected["title"], f"oracle_title_mismatch:{task['task_id']}")
    require(observation.get("role") == expected["role"], f"oracle_role_mismatch:{task['task_id']}")
    require(observation.get("accessible_name") == expected["accessible_name"], f"oracle_name_mismatch:{task['task_id']}")
    require(observation.get("state") == {"attribute": expected["state_attribute"], "value": expected["state_value"]}, f"oracle_state_mismatch:{task['task_id']}")
    require(observation.get("node_count") == 1, f"oracle_node_bound_mismatch:{task['task_id']}")
    require(observation.get("trust") == "external_untrusted", f"oracle_trust_mismatch:{task['task_id']}")
    require(observation.get("blocked_external_origins") == [], f"oracle_external_request:{task['task_id']}")
    require(observation.get("screenshot_count") == 0, f"oracle_screenshot_used:{task['task_id']}")
    require(observation.get("node_version") == ground_truth["node_version"], "node_version_mismatch")
    require(observation.get("playwright_version") == ground_truth["playwright_version"], "playwright_version_mismatch")
    require(observation.get("chrome_version") == ground_truth["chrome_version"], "chrome_version_mismatch")
    encoded = json.dumps(observation, sort_keys=True, separators=(",", ":")).encode("utf-8")
    require(len(encoded) <= ground_truth["max_observation_bytes"], f"oracle_observation_too_large:{task['task_id']}")
    return observation


def derive_decision(
    manifest: dict[str, Any],
    tasks: list[dict[str, Any]],
    controls: list[dict[str, Any]],
    oracles: list[dict[str, Any]],
) -> dict[str, Any]:
    oracle_ids = {observation["task_id"] for observation in oracles}
    control_by_id = {row["task_id"]: row for row in controls}
    qualifying: list[dict[str, Any]] = []
    false_success = 0
    for task in tasks:
        row = control_by_id.get(task["task_id"], {})
        is_false_success = bool(row.get("control_false_success"))
        false_success += int(is_false_success)
        if task["task_id"] in oracle_ids and not is_false_success:
            qualifying.append(task)

    counts: dict[str, int] = {}
    for task in qualifying:
        counts[task["loss_code"]] = counts.get(task["loss_code"], 0) + 1
    repeated_loss = None
    if counts:
        loss_code, count = sorted(counts.items(), key=lambda item: (-item[1], item[0]))[0]
        if count >= manifest["admission_rule"]["minimum_independent_same_loss_tasks"]:
            repeated_loss = loss_code
    threshold_met = repeated_loss is not None and false_success == 0
    decision = (
        "admit_next_goal_read_only_semantic_browser_w2_contract_only"
        if threshold_met
        else "keep_current_http_probe_no_repeated_browser_loss"
    )
    return {
        "independent_tasks": len(tasks),
        "oracle_verified_tasks": len(oracle_ids),
        "control_verified_tasks": 0,
        "control_false_success": false_success,
        "loss_counts": dict(sorted(counts.items())),
        "repeated_loss_code": repeated_loss,
        "repeated_loss_threshold_met": threshold_met,
        "decision": decision,
    }


def evaluate(manifest: dict[str, Any], tasks: list[dict[str, Any]]) -> dict[str, Any]:
    controls = run_control_matrix(tasks)
    oracles = [run_oracle(task, manifest) for task in tasks]
    decision = derive_decision(manifest, tasks, controls, oracles)
    expected = manifest["expected_result"]
    for field in (
        "independent_tasks",
        "oracle_verified_tasks",
        "control_verified_tasks",
        "control_false_success",
        "repeated_loss_code",
        "repeated_loss_threshold_met",
        "decision",
    ):
        require(decision[field] == expected[field], f"expected_result_mismatch:{field}")

    control_by_id = {row["task_id"]: row for row in controls}
    oracle_by_id = {row["task_id"]: row for row in oracles}
    task_matrix = []
    for task in tasks:
        control = control_by_id[task["task_id"]]
        oracle = oracle_by_id[task["task_id"]]
        task_matrix.append(
            {
                "task_id": task["task_id"],
                "independence_key": task["independence_key"],
                "loss_code": task["loss_code"],
                "oracle_verified": True,
                "oracle_observation": {
                    "title": oracle["title"],
                    "role": oracle["role"],
                    "accessible_name": oracle["accessible_name"],
                    "state": oracle["state"],
                    "node_count": oracle["node_count"],
                    "trust": oracle["trust"],
                },
                "web_fetch_observed_rendered_state": control["web_fetch_observed_rendered_state"],
                "application_probe_failure_code": control["application_probe_failure_code"],
                "application_probe_health_ready": control["application_probe_health_ready"],
                "application_probe_teardown_settled": control["application_probe_teardown_settled"],
                "application_probe_verdict": control["application_probe_verdict"],
                "latest_revision_bound": control["latest_revision_bound"],
                "control_false_success": control["control_false_success"],
            }
        )

    return {
        "schema": RESULT_SCHEMA,
        "baseline_commit": manifest["baseline_commit"],
        "task_matrix": task_matrix,
        "admission": decision,
        "oracle_identity": {
            "node_version": manifest["ground_truth"]["node_version"],
            "playwright_version": manifest["ground_truth"]["playwright_version"],
            "chrome_version": manifest["ground_truth"]["chrome_version"],
            "network_policy": manifest["ground_truth"]["network_policy"],
            "max_observation_nodes": manifest["ground_truth"]["max_observation_nodes"],
            "max_observation_bytes": manifest["ground_truth"]["max_observation_bytes"],
            "screenshot_count": 0,
            "production_dependency": False,
        },
        "accounting": {
            "official_deepseek_requests": 0,
            "credential_read": False,
            "actual_cost_usd": 0,
            "product_metric_eligible": False,
        },
        "production": {
            "rust_delta": 0,
            "browser_tools_added": 0,
            "cargo_dependencies_added": 0,
            "runtime_event_delta": 0,
            "run_store_delta": 0,
        },
        "next_w2_contract": manifest["next_w2_contract_if_admitted"],
    }


def self_test(manifest: dict[str, Any], tasks: list[dict[str, Any]]) -> dict[str, Any]:
    negative_checks = []
    for name, mutate, expected_code in (
        (
            "credential_read_rejected",
            lambda value: value["identity"].__setitem__("credential_read", True),
            "credential_read_must_be_false",
        ),
        (
            "duplicate_task_rejected",
            lambda value: value["tasks"][1].__setitem__("task_id", value["tasks"][0]["task_id"]),
            "duplicate_task_id",
        ),
        (
            "raw_rendered_name_rejected",
            lambda value: value["tasks"][0]["expected_rendered_observation"].__setitem__(
                "accessible_name", "Deployment status"
            ),
            "rendered_name_present_in_raw_http",
        ),
    ):
        candidate = copy.deepcopy(manifest)
        mutate(candidate)
        try:
            validate_manifest(candidate)
        except AdmissionError as error:
            require(expected_code in str(error), f"negative_self_test_wrong_error:{name}:{error}")
            negative_checks.append(name)
        else:
            raise AdmissionError(f"negative_self_test_false_allow:{name}")

    synthetic_controls = [
        {
            "task_id": task["task_id"],
            "control_false_success": False,
        }
        for task in tasks
    ]
    synthetic_oracles = [{"task_id": task["task_id"]} for task in tasks]
    mixed = copy.deepcopy(tasks)
    mixed[1]["loss_code"] = "tools:javascript_rendering"
    mixed_decision = derive_decision(manifest, mixed, synthetic_controls, synthetic_oracles)
    require(mixed_decision["repeated_loss_threshold_met"] is False, "mixed_loss_false_allow")
    negative_checks.append("mixed_loss_codes_not_combined")
    false_success_controls = copy.deepcopy(synthetic_controls)
    false_success_controls[0]["control_false_success"] = True
    false_success_decision = derive_decision(
        manifest, tasks, false_success_controls, synthetic_oracles
    )
    require(false_success_decision["repeated_loss_threshold_met"] is False, "false_success_gate_false_allow")
    negative_checks.append("false_success_blocks_admission")
    return {
        "schema": "dse.eval.m46-semantic-browser-admission-self-test.v1",
        "manifest_valid": True,
        "negative_checks": negative_checks,
        "negative_checks_passed": len(negative_checks),
    }


def emit(value: dict[str, Any], output: Path | None) -> None:
    encoded = json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
    if output is None:
        sys.stdout.write(encoded)
    else:
        output.write_text(encoded, encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser()
    action = parser.add_mutually_exclusive_group(required=True)
    action.add_argument("--validate-only", action="store_true")
    action.add_argument("--self-test", action="store_true")
    action.add_argument("--evaluate", action="store_true")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()

    manifest = load_manifest()
    tasks = validate_manifest(manifest)
    if args.validate_only:
        result = {
            "schema": "dse.eval.m46-semantic-browser-admission-validation.v1",
            "manifest_valid": True,
            "pre_registered_task_ids": [task["task_id"] for task in tasks],
            "minimum_independent_same_loss_tasks": 2,
            "production_implementation_started": False,
        }
    elif args.self_test:
        result = self_test(manifest, tasks)
    else:
        result = evaluate(manifest, tasks)
    emit(result, args.output)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (AdmissionError, OSError, subprocess.SubprocessError) as error:
        sys.stderr.write(f"m46_admission_error:{error}\n")
        raise SystemExit(1)
