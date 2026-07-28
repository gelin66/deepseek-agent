#!/usr/bin/env python3
"""Evaluate the post-W3 single-family browser interaction admission gate.

The evaluator is credential-free, exact-loopback-only, and production-read-only.
It runs current web_fetch/browser_navigate/browser_click controls, then one
eval-only semantic fill per pre-registered task. It cannot add a production
browser tool or durable browser state.
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
from typing import Any, Callable


ROOT = Path(__file__).resolve().parents[1]
MANIFEST_PATH = ROOT / "eval/manifests/m46-post-w3-interaction-admission-v1.json"
EXPECTED_SCHEMA = "dse.eval.m46-post-w3-interaction-admission.v1"
RESULT_SCHEMA = "dse.eval.m46-post-w3-interaction-admission-result.v1"
CONTROL_MARKER = "M46_POST_W3_CONTROL_MATRIX="
CONTROL_TEST = "current_w3_root_cannot_complete_pre_registered_fill_tasks"
REPLAY_TEST = "m46_w3_agent_click_commits_and_sqlite_reopen_never_clicks_again"
TEARDOWN_TEST = (
    "semantic_browser::tests::"
    "pinned_cft_reads_js_only_ax_dom_and_retains_only_exact_local_session"
)


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


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while chunk := source.read(1024 * 1024):
            digest.update(chunk)
    return "sha256:" + digest.hexdigest()


def validate_manifest(manifest: dict[str, Any]) -> list[dict[str, Any]]:
    require(manifest.get("schema") == EXPECTED_SCHEMA, "manifest_schema_mismatch")
    require(
        manifest.get("baseline_commit")
        == "64b83a21b7859e38ca7ce43a4a035c6337702d36",
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
        == [
            "current_production_web_fetch",
            "current_production_browser_navigate",
            "current_production_browser_click",
        ],
        "control_surface_mismatch",
    )
    require(identity.get("current_model_catalog_size") == 15, "catalog_size_mismatch")
    require(
        identity.get("current_browser_action_tools") == ["browser_click"],
        "current_action_surface_mismatch",
    )
    require(identity.get("official_deepseek_requests") == 0, "official_requests_must_be_zero")
    require(identity.get("credential_read") is False, "credential_read_must_be_false")
    require(identity.get("external_network_allowed") is False, "external_network_must_be_false")
    require(identity.get("maximum_formal_runs") == 1, "formal_run_limit_mismatch")
    require(identity.get("maximum_reruns") == 0, "maximum_reruns_must_be_zero")
    require(identity.get("actual_cost_usd") == 0, "actual_cost_must_be_zero")
    require(identity.get("product_metric_eligible") is False, "product_metric_must_be_false")

    rule = manifest.get("admission_rule")
    require(isinstance(rule, dict), "admission_rule_missing")
    require(rule.get("exact_task_count") == 2, "task_count_rule_must_be_two")
    require(rule.get("minimum_independent_same_loss_tasks") == 2, "loss_threshold_must_be_two")
    require(rule.get("allowed_loss_codes") == ["tools:browser_interaction"], "loss_code_rule_mismatch")
    require(
        rule.get("candidate_action_families") == ["fill", "press", "wait"],
        "candidate_action_families_mismatch",
    )
    require(rule.get("mixed_action_families_may_not_be_combined") is True, "mixed_action_rule_missing")
    require(rule.get("oracle_must_pass") is True, "oracle_gate_missing")
    require(rule.get("control_false_success_must_equal") == 0, "false_success_gate_mismatch")
    require(rule.get("teardown_and_replay_must_pass") is True, "regression_gate_missing")

    ground_truth = manifest.get("ground_truth")
    require(isinstance(ground_truth, dict), "ground_truth_missing")
    require(
        ground_truth.get("kind") == "eval_only_exact_role_name_fill_oracle",
        "oracle_kind_mismatch",
    )
    require(ground_truth.get("screenshots") is False, "screenshots_must_be_false")
    require(ground_truth.get("coordinates") is False, "coordinates_must_be_false")
    require(ground_truth.get("production_dependency") is False, "oracle_must_not_be_production")
    require(ground_truth.get("max_observation_nodes") == 2, "observation_node_bound_mismatch")
    max_bytes = ground_truth.get("max_observation_bytes")
    require(isinstance(max_bytes, int) and 0 < max_bytes <= 4096, "observation_byte_bound_invalid")
    for path_field, sha_field in (
        ("runner", "runner_sha256"),
        ("node_runner", "node_runner_sha256"),
        ("control_caller", "control_caller_sha256"),
    ):
        path = repository_file(ground_truth.get(path_field, ""))
        require(sha256_file(path) == ground_truth.get(sha_field), f"{sha_field}_mismatch")
    chrome_path = Path(ground_truth.get("chrome_path", ""))
    require(chrome_path.is_file(), "pinned_chrome_missing")
    require(
        sha256_file(chrome_path) == ground_truth.get("chrome_executable_sha256"),
        "pinned_chrome_sha256_mismatch",
    )

    shared = manifest.get("shared_fixture")
    require(isinstance(shared, dict), "shared_fixture_missing")
    server = repository_file(shared.get("server_path", ""))
    require(sha256_file(server) == shared.get("server_sha256"), "server_sha256_mismatch")

    tasks = manifest.get("tasks")
    require(isinstance(tasks, list) and len(tasks) == 2, "task_count_must_be_two")
    task_ids: set[str] = set()
    independence_keys: set[str] = set()
    families: set[str] = set()
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
        require(task.get("loss_code") == "tools:browser_interaction", f"loss_code_mismatch:{task_id}")
        require(
            isinstance(task.get("task_contract"), str) and bool(task["task_contract"]),
            f"task_contract_missing:{task_id}",
        )
        fixture = repository_file(task.get("fixture_path", ""))
        require(sha256_file(fixture) == task.get("fixture_sha256"), f"fixture_sha256_mismatch:{task_id}")
        source = fixture.read_text(encoding="utf-8")
        require("<script>" in source and "</script>" in source, f"js_fixture_missing:{task_id}")
        require(source.count("{{DSE_APPLICATION_PROBE_LEASE}}") == 1, f"lease_placeholder_mismatch:{task_id}")

        action = task.get("action")
        require(isinstance(action, dict), f"action_missing:{task_id}")
        family = action.get("family")
        require(family in rule["candidate_action_families"], f"action_family_invalid:{task_id}")
        families.add(family)
        require(action.get("selector") == "exact_role_name", f"action_selector_invalid:{task_id}")
        for field in ("role", "accessible_name", "value"):
            require(
                isinstance(action.get(field), str) and bool(action[field]),
                f"action_field_missing:{task_id}:{field}",
            )
            require(action[field] not in source, f"action_literal_present_in_raw_http:{task_id}:{field}")

        expected = task.get("expected_post_action_observation")
        require(isinstance(expected, dict), f"expected_observation_missing:{task_id}")
        for field in ("title", "role", "accessible_name", "state_attribute", "state_value"):
            require(
                isinstance(expected.get(field), str) and bool(expected[field]),
                f"expected_field_missing:{task_id}:{field}",
            )
        for field in ("accessible_name", "state_value"):
            require(
                expected[field] not in source,
                f"post_action_literal_present_in_raw_http:{task_id}:{field}",
            )

        control = task.get("expected_control")
        require(isinstance(control, dict), f"expected_control_missing:{task_id}")
        require(control.get("web_fetch_post_action_state") == "absent", f"web_fetch_control_mismatch:{task_id}")
        require(control.get("browser_initial_action_target") == "present", f"browser_target_control_mismatch:{task_id}")
        require(control.get("browser_post_action_state") == "absent", f"browser_result_control_mismatch:{task_id}")
        require(control.get("candidate_target_refs") == 0, f"browser_ref_control_mismatch:{task_id}")
        require(control.get("candidate_action_tool_visible") is False, f"browser_action_control_mismatch:{task_id}")
        require(control.get("unknown_action_side_effect") == "not_applied", f"browser_action_side_effect_mismatch:{task_id}")

    require(len(families) == 1, "mixed_action_families_in_manifest")
    selected_family = next(iter(families))
    next_contract = manifest.get("next_goal_if_admitted")
    require(isinstance(next_contract, dict), "next_goal_contract_missing")
    require(next_contract.get("owner") == "crates/tools", "next_goal_owner_mismatch")
    require(next_contract.get("action_family") == selected_family, "next_goal_action_family_mismatch")
    require(next_contract.get("production_implementation_in_this_audit") is False, "production_must_not_start")
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


def run_checked(command: list[str], environment: dict[str, str], timeout: int) -> str:
    completed = subprocess.run(
        command,
        cwd=ROOT,
        env=environment,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        timeout=timeout,
        check=False,
    )
    require(completed.returncode == 0, f"command_failed:{' '.join(command)}:{completed.stdout[-4000:]}")
    return completed.stdout


def verify_production_delta(manifest: dict[str, Any]) -> None:
    head = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        check=True,
    ).stdout.strip()
    require(head == manifest["baseline_commit"], "head_is_not_clean_baseline")
    status = subprocess.run(
        ["git", "status", "--porcelain=v1", "-z", "--untracked-files=all"],
        cwd=ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=True,
    ).stdout.decode("utf-8")
    allowed_prefixes = (
        "crates/tools/tests/m46_post_w3_interaction_admission.rs",
        "eval/fixtures/m46-js-application-visibility/interaction-release-channel-fill.html",
        "eval/fixtures/m46-js-application-visibility/interaction-test-filter-fill.html",
        "eval/manifests/m46-post-w3-interaction-admission-v1.json",
        "eval/summaries/m46-post-w3-interaction-admission-2026-07-28.md",
        "scripts/eval-m46-post-w3-interaction-admission.py",
        "scripts/eval-m46-post-w3-interaction-oracle.cjs",
        "docs/architecture/CURRENT_CODEWHALE.md",
        "docs/decisions/0015-native-web-retrieval-and-semantic-browser.md",
        "docs/product/EVALUATION.md",
        "docs/product/ROADMAP.md",
    )
    for record in filter(None, status.split("\0")):
        path = record[3:].split(" -> ")[-1]
        require(path in allowed_prefixes, f"out_of_scope_delta_detected:{path}")
        production_path = (
            path in {"Cargo.toml", "Cargo.lock"}
            or (path.startswith("crates/") and path.endswith("/Cargo.toml"))
            or (path.startswith("crates/") and "/src/" in path)
        )
        require(not production_path, f"production_delta_detected:{path}")
    frozen = (
        "eval/manifests/m46-browser-interaction-admission-v1.json",
        "eval/summaries/m46-browser-interaction-admission-2026-07-28.md",
    )
    unchanged = subprocess.run(
        ["git", "diff", "--quiet", "--", *frozen],
        cwd=ROOT,
        check=False,
    )
    require(unchanged.returncode == 0, "prior_frozen_evidence_changed")


def run_control_matrix(tasks: list[dict[str, Any]], manifest: dict[str, Any]) -> list[dict[str, Any]]:
    environment = safe_base_environment()
    environment["CARGO_NET_OFFLINE"] = "true"
    environment["CARGO_INCREMENTAL"] = "0"
    environment["DSE_CHROME_FOR_TESTING_PATH"] = manifest["ground_truth"]["chrome_path"]
    output = run_checked(
        [
            "cargo",
            "test",
            "-p",
            "dse-tools",
            "--test",
            "m46_post_w3_interaction_admission",
            "--locked",
            CONTROL_TEST,
            "--",
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ],
        environment,
        600,
    )
    marker_lines = [line for line in output.splitlines() if CONTROL_MARKER in line]
    require(len(marker_lines) == 1, "production_control_marker_missing")
    try:
        matrix = json.loads(marker_lines[0].split(CONTROL_MARKER, 1)[1])
    except json.JSONDecodeError as error:
        raise AdmissionError(f"production_control_json_invalid:{error}") from error
    require(isinstance(matrix, list) and len(matrix) == len(tasks), "production_control_count_mismatch")
    require([row.get("task_id") for row in matrix] == [task["task_id"] for task in tasks], "production_control_task_order_mismatch")
    for row, task in zip(matrix, tasks):
        task_id = task["task_id"]
        require(row.get("independence_key") == task["independence_key"], f"control_independence_mismatch:{task_id}")
        require(row.get("loss_code") == "tools:browser_interaction", f"control_loss_mismatch:{task_id}")
        require(row.get("action_family") == task["action"]["family"], f"control_action_mismatch:{task_id}")
        require(row.get("root_permission_mode") == "agent", f"root_actor_mismatch:{task_id}")
        require(row.get("http_observed_post_action_state") is False, f"http_false_success:{task_id}")
        require(row.get("browser_initial_action_target_observed") is True, f"browser_target_absent:{task_id}")
        require(row.get("browser_observed_post_action_state") is False, f"browser_false_success:{task_id}")
        require(row.get("browser_fill_target_refs_returned") == 0, f"fill_ref_surface_changed:{task_id}")
        require(row.get("browser_fill_tool_visible") is False, f"fill_tool_surface_changed:{task_id}")
        require(row.get("browser_fill_dispatch_failure_code") == "unknown_tool", f"fill_dispatch_mismatch:{task_id}")
        require(row.get("browser_fill_dispatch_side_effect") == "not_applied", f"fill_side_effect_mismatch:{task_id}")
        require(row.get("browser_session_live_before_host_drop") is True, f"browser_session_missing:{task_id}")
        require(row.get("trust") == "external_untrusted", f"browser_trust_mismatch:{task_id}")
        require(row.get("control_verified") is False, f"control_verified_unexpectedly:{task_id}")
        require(row.get("control_false_success") is False, f"control_false_success:{task_id}")
    return matrix


def run_regression(test_filter: str, manifest: dict[str, Any]) -> bool:
    environment = safe_base_environment()
    environment["CARGO_NET_OFFLINE"] = "true"
    environment["CARGO_INCREMENTAL"] = "0"
    environment["DSE_CHROME_FOR_TESTING_PATH"] = manifest["ground_truth"]["chrome_path"]
    command = ["cargo", "test", "-p", "dse-app", test_filter, "--locked", "--", "--nocapture"]
    if test_filter == TEARDOWN_TEST:
        command = [
            "cargo", "test", "-p", "dse-tools", test_filter, "--locked", "--",
            "--ignored", "--nocapture", "--test-threads=1",
        ]
    output = run_checked(command, environment, 600)
    require(test_filter.split("::")[-1] in output and "1 passed" in output, f"regression_not_observed:{test_filter}")
    return True


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
    process = subprocess.Popen(
        [
            "/usr/bin/python3",
            "-I",
            "-B",
            str(server_path),
            f"dse-application-probe:m46-post-w3-{task['task_id']}",
        ],
        cwd=server_path.parent,
        env={
            "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
            "HOST": "127.0.0.1",
            "PORT": str(port),
            "M46_FIXTURE_HTML": fixture_path.name,
        },
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        start_new_session=True,
    )
    try:
        wait_for_health(port, process)
        with tempfile.TemporaryDirectory(prefix="dse-m46-post-w3-oracle-home-") as oracle_home:
            completed = subprocess.run(
                ["node", str(repository_file(ground_truth["node_runner"]))],
                cwd=ROOT,
                env={
                    "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
                    "HOME": oracle_home,
                    "NODE_PATH": ground_truth["playwright_node_path"],
                    "M46_POST_W3_TASK_ID": task["task_id"],
                    "M46_POST_W3_TARGET_URL": origin + "/",
                    "M46_POST_W3_ALLOWED_ORIGIN": origin,
                    "M46_POST_W3_CHROME_PATH": ground_truth["chrome_path"],
                    "M46_POST_W3_ACTION": json.dumps(task["action"], sort_keys=True, separators=(",", ":")),
                    "M46_POST_W3_EXPECTED": json.dumps(task["expected_post_action_observation"], sort_keys=True, separators=(",", ":")),
                    "M46_POST_W3_MAX_BYTES": str(ground_truth["max_observation_bytes"]),
                },
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

    action = task["action"]
    expected = task["expected_post_action_observation"]
    require(observation.get("task_id") == task["task_id"], f"oracle_task_mismatch:{task['task_id']}")
    require(observation.get("title") == expected["title"], f"oracle_title_mismatch:{task['task_id']}")
    require(observation.get("action") == {
        "family": "fill",
        "selector": "exact_role_name",
        "role": action["role"],
        "accessible_name": action["accessible_name"],
        "value": action["value"],
        "count": 1,
    }, f"oracle_action_mismatch:{task['task_id']}")
    require(observation.get("result") == {
        "node_count": 1,
        "role": expected["role"],
        "accessible_name": expected["accessible_name"],
        "state": {"attribute": expected["state_attribute"], "value": expected["state_value"]},
    }, f"oracle_result_mismatch:{task['task_id']}")
    require(observation.get("trust") == "external_untrusted", f"oracle_trust_mismatch:{task['task_id']}")
    require(observation.get("blocked_external_origins") == [], f"oracle_external_request:{task['task_id']}")
    require(observation.get("screenshot_count") == 0, f"oracle_screenshot_used:{task['task_id']}")
    require(observation.get("coordinate_action_count") == 0, f"oracle_coordinate_used:{task['task_id']}")
    require(observation.get("teardown") == {"context_closed": True, "browser_closed": True}, f"oracle_teardown_failed:{task['task_id']}")
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
    replay_passed: bool,
    teardown_passed: bool,
) -> dict[str, Any]:
    oracle_ids = {observation["task_id"] for observation in oracles}
    control_by_id = {row["task_id"]: row for row in controls}
    qualifying: list[dict[str, Any]] = []
    false_success = 0
    for task in tasks:
        row = control_by_id.get(task["task_id"], {})
        is_false_success = bool(
            row.get("control_false_success")
            or row.get("http_observed_post_action_state")
            or row.get("browser_observed_post_action_state")
            or row.get("control_verified")
        )
        false_success += int(is_false_success)
        if (
            task["task_id"] in oracle_ids
            and not is_false_success
            and row.get("browser_initial_action_target_observed") is True
            and row.get("browser_fill_target_refs_returned") == 0
            and row.get("browser_fill_tool_visible") is False
            and row.get("browser_fill_dispatch_failure_code") == "unknown_tool"
            and row.get("browser_fill_dispatch_side_effect") == "not_applied"
        ):
            qualifying.append(task)

    counts: dict[str, int] = {}
    for task in qualifying:
        key = f"{task['loss_code']}:{task['action']['family']}"
        counts[key] = counts.get(key, 0) + 1
    repeated_key = None
    if counts:
        key, count = sorted(counts.items(), key=lambda item: (-item[1], item[0]))[0]
        if count >= manifest["admission_rule"]["minimum_independent_same_loss_tasks"]:
            repeated_key = key
    regressions_passed = bool(replay_passed and teardown_passed)
    threshold_met = repeated_key is not None and false_success == 0 and regressions_passed
    repeated_family = repeated_key.rsplit(":", 1)[-1] if threshold_met else None
    return {
        "independent_tasks": len(tasks),
        "oracle_verified_tasks": len(oracle_ids),
        "control_verified_tasks": 0,
        "control_false_success": false_success,
        "loss_action_counts": dict(sorted(counts.items())),
        "repeated_loss_code": "tools:browser_interaction" if threshold_met else None,
        "repeated_action_family": repeated_family,
        "repeated_loss_threshold_met": threshold_met,
        "host_teardown_regression_passed": teardown_passed,
        "agent_application_replay_regression_passed": replay_passed,
        "decision": (
            f"admit_next_goal_ref_based_browser_{repeated_family}_w3_1_contract_only"
            if threshold_met
            else "hold_no_repeated_loss"
        ),
    }


def synthetic_controls(tasks: list[dict[str, Any]]) -> list[dict[str, Any]]:
    return [
        {
            "task_id": task["task_id"],
            "http_observed_post_action_state": False,
            "browser_initial_action_target_observed": True,
            "browser_observed_post_action_state": False,
            "browser_fill_target_refs_returned": 0,
            "browser_fill_tool_visible": False,
            "browser_fill_dispatch_failure_code": "unknown_tool",
            "browser_fill_dispatch_side_effect": "not_applied",
            "control_verified": False,
            "control_false_success": False,
        }
        for task in tasks
    ]


def expect_admission_false(
    manifest: dict[str, Any],
    tasks: list[dict[str, Any]],
    controls: list[dict[str, Any]],
    oracles: list[dict[str, Any]],
    replay: bool = True,
    teardown: bool = True,
) -> None:
    require(
        derive_decision(manifest, tasks, controls, oracles, replay, teardown)[
            "repeated_loss_threshold_met"
        ] is False,
        "negative_self_test_false_allow",
    )


def self_test(manifest: dict[str, Any], tasks: list[dict[str, Any]]) -> dict[str, Any]:
    negative_checks: list[str] = []
    validation_cases: tuple[tuple[str, Callable[[dict[str, Any]], None], str], ...] = (
        (
            "credential_read_rejected",
            lambda value: value["identity"].__setitem__("credential_read", True),
            "credential_read_must_be_false",
        ),
        (
            "duplicate_independence_rejected",
            lambda value: value["tasks"][1].__setitem__("independence_key", value["tasks"][0]["independence_key"]),
            "independence_key_invalid",
        ),
        (
            "mixed_action_family_rejected",
            lambda value: value["tasks"][1]["action"].__setitem__("family", "press"),
            "mixed_action_families_in_manifest",
        ),
        (
            "raw_target_literal_rejected",
            lambda value: value["tasks"][0]["action"].__setitem__("accessible_name", "release-channel"),
            "action_literal_present_in_raw_http",
        ),
        (
            "raw_fill_value_rejected",
            lambda value: value["tasks"][0]["action"].__setitem__("value", "targetValue"),
            "action_literal_present_in_raw_http",
        ),
    )
    for name, mutate, expected_code in validation_cases:
        candidate = copy.deepcopy(manifest)
        mutate(candidate)
        try:
            validate_manifest(candidate)
        except AdmissionError as error:
            require(expected_code in str(error), f"negative_self_test_wrong_error:{name}:{error}")
            negative_checks.append(name)
        else:
            raise AdmissionError(f"negative_self_test_false_allow:{name}")

    controls = synthetic_controls(tasks)
    oracles = [{"task_id": task["task_id"]} for task in tasks]
    false_success = copy.deepcopy(controls)
    false_success[0]["browser_observed_post_action_state"] = True
    expect_admission_false(manifest, tasks, false_success, oracles)
    negative_checks.append("false_success_blocks_admission")
    missing_target = copy.deepcopy(controls)
    missing_target[0]["browser_initial_action_target_observed"] = False
    expect_admission_false(manifest, tasks, missing_target, oracles)
    negative_checks.append("missing_target_blocks_admission")
    candidate_visible = copy.deepcopy(controls)
    candidate_visible[0]["browser_fill_tool_visible"] = True
    expect_admission_false(manifest, tasks, candidate_visible, oracles)
    negative_checks.append("changed_control_surface_blocks_admission")
    expect_admission_false(manifest, tasks, controls, oracles[:1])
    negative_checks.append("one_oracle_task_does_not_meet_threshold")
    expect_admission_false(manifest, tasks, controls, oracles, replay=False)
    negative_checks.append("replay_regression_blocks_admission")
    expect_admission_false(manifest, tasks, controls, oracles, teardown=False)
    negative_checks.append("teardown_regression_blocks_admission")
    return {
        "schema": "dse.eval.m46-post-w3-interaction-admission-self-test.v1",
        "manifest_valid": True,
        "negative_checks": negative_checks,
        "negative_checks_passed": len(negative_checks),
    }


def evaluate(manifest: dict[str, Any], tasks: list[dict[str, Any]]) -> dict[str, Any]:
    verify_production_delta(manifest)
    controls = run_control_matrix(tasks, manifest)
    replay_passed = run_regression(REPLAY_TEST, manifest)
    teardown_passed = run_regression(TEARDOWN_TEST, manifest)
    oracles = [run_oracle(task, manifest) for task in tasks]
    admission = derive_decision(
        manifest, tasks, controls, oracles, replay_passed, teardown_passed
    )
    expected = manifest["expected_result"]
    for field in (
        "independent_tasks",
        "oracle_verified_tasks",
        "control_verified_tasks",
        "control_false_success",
        "repeated_loss_code",
        "repeated_action_family",
        "repeated_loss_threshold_met",
        "host_teardown_regression_passed",
        "agent_application_replay_regression_passed",
        "decision",
    ):
        require(admission[field] == expected[field], f"expected_result_mismatch:{field}")

    control_by_id = {row["task_id"]: row for row in controls}
    oracle_by_id = {row["task_id"]: row for row in oracles}
    matrix = []
    for task in tasks:
        control = control_by_id[task["task_id"]]
        oracle = oracle_by_id[task["task_id"]]
        matrix.append({
            "task_id": task["task_id"],
            "independence_key": task["independence_key"],
            "loss_code": task["loss_code"],
            "action_family": task["action"]["family"],
            "oracle_verified": True,
            "oracle_action": oracle["action"],
            "oracle_result": oracle["result"],
            "http_observed_post_action_state": control["http_observed_post_action_state"],
            "browser_initial_action_target_observed": control["browser_initial_action_target_observed"],
            "browser_observed_post_action_state": control["browser_observed_post_action_state"],
            "browser_fill_target_refs_returned": control["browser_fill_target_refs_returned"],
            "browser_fill_tool_visible": control["browser_fill_tool_visible"],
            "browser_fill_dispatch_failure_code": control["browser_fill_dispatch_failure_code"],
            "browser_fill_dispatch_side_effect": control["browser_fill_dispatch_side_effect"],
            "host_teardown_regression_passed": teardown_passed,
            "agent_application_replay_regression_passed": replay_passed,
            "control_verified": control["control_verified"],
            "control_false_success": control["control_false_success"],
        })

    return {
        "schema": RESULT_SCHEMA,
        "baseline_commit": manifest["baseline_commit"],
        "task_matrix": matrix,
        "admission": admission,
        "oracle_identity": {
            "node_version": manifest["ground_truth"]["node_version"],
            "playwright_version": manifest["ground_truth"]["playwright_version"],
            "chrome_version": manifest["ground_truth"]["chrome_version"],
            "chrome_executable_sha256": manifest["ground_truth"]["chrome_executable_sha256"],
            "network_policy": manifest["ground_truth"]["network_policy"],
            "max_observation_nodes": manifest["ground_truth"]["max_observation_nodes"],
            "max_observation_bytes": manifest["ground_truth"]["max_observation_bytes"],
            "action_count_per_task": 1,
            "screenshot_count": 0,
            "coordinate_action_count": 0,
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
            "cargo_dependencies_added": 0,
            "browser_action_tools_added": 0,
            "deepseek_wire_delta": 0,
            "model_visible_prompt_delta": 0,
            "runtime_event_delta": 0,
            "run_store_delta": 0,
            "state_schema_delta": 0,
            "browser_session_delta": 0,
        },
        "next_goal": manifest["next_goal_if_admitted"],
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
            "schema": "dse.eval.m46-post-w3-interaction-admission-validation.v1",
            "manifest_valid": True,
            "pre_registered_task_ids": [task["task_id"] for task in tasks],
            "pre_registered_action_family": tasks[0]["action"]["family"],
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
        sys.stderr.write(f"m46_post_w3_interaction_admission_error:{error}\n")
        raise SystemExit(1)
