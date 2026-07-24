#!/usr/bin/env python3
"""Verify the M8-N V15 release-scope successor without credentials or network.

This one-shot Harness owns no model loop, prompt builder, tool, verifier,
accounting projector, release installer or state reducer. It checks immutable
evidence, invokes exact production tests, and drives the existing delivery
owner against a locked/offline artifact.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import time


ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "eval/manifests/m8-n-v15-release-scope-successor-v1.json"
HARNESS_TEST = ROOT / "scripts/test-eval-m8n-v15-release-scope.py"
EXPECTED_SCHEMA = "codewhale.eval.m8-n-v15-release-scope-successor.v1"
TARGET_DIR = "/private/tmp/codewhale-m8n-target"
PROMPT_PATH = "crates/context/src/prompts/constitution.md"


class HarnessError(RuntimeError):
    pass


def require(condition: bool, code: str) -> None:
    if not condition:
        raise HarnessError(code)


def canonical_json(value: object) -> bytes:
    return json.dumps(
        value, ensure_ascii=False, sort_keys=True, separators=(",", ":")
    ).encode("utf-8")


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def git(*args: str, check: bool = True) -> str:
    completed = subprocess.run(
        ["git", *args],
        cwd=ROOT,
        check=check,
        text=True,
        capture_output=True,
    )
    return completed.stdout.strip()


def git_show_bytes(revision: str, path: str) -> bytes:
    return subprocess.run(
        ["git", "show", f"{revision}:{path}"],
        cwd=ROOT,
        check=True,
        capture_output=True,
    ).stdout


def read_text(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def load_manifest() -> tuple[dict, str]:
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    require(manifest.get("schema") == EXPECTED_SCHEMA, "manifest_schema_mismatch")
    frozen_assets = manifest.get("frozen_assets", {})
    require(
        frozen_assets.get("harness_sha256") == sha256_file(Path(__file__)),
        "harness_sha_mismatch",
    )
    require(
        frozen_assets.get("harness_test_sha256") == sha256_file(HARNESS_TEST),
        "harness_test_sha_mismatch",
    )
    baseline = manifest.get("baseline", {})
    require(
        baseline.get("revision")
        == "21200ccf0b4118793f1ad4e64acc973c25741e08",
        "baseline_revision_mismatch",
    )
    require(
        git("rev-parse", f"{baseline['revision']}^{{tree}}") == baseline["tree"],
        "baseline_tree_mismatch",
    )
    execution = manifest.get("execution", {})
    require(execution.get("maximum_reruns") == 0, "maximum_reruns_must_be_zero")
    require(
        execution.get("cargo_target_dir") == TARGET_DIR,
        "cargo_target_dir_mismatch",
    )
    require(execution.get("result_mode") == "0600", "result_mode_must_be_0600")
    for field in (
        "credential_read",
        "official_api_requests_allowed",
        "external_network_allowed",
    ):
        require(execution.get(field) is False, f"{field}_must_be_false")
    decision = manifest.get("successor_decision_contract", {})
    require(
        decision.get("material_model_visible_delta") is False,
        "material_model_visible_delta_must_be_false",
    )
    require(
        decision.get("live_api_admission")
        == "inadmissible_no_material_treatment",
        "live_api_admission_mismatch",
    )
    gates = manifest.get("current_release_gates")
    require(isinstance(gates, list) and len(gates) == 7, "seven_gates_required")
    require(
        [gate.get("id") for gate in gates]
        == ["P01", "P02", "P03", "P04", "P05", "P06", "P07"],
        "gate_ids_mismatch",
    )
    return manifest, sha256_bytes(canonical_json(manifest))


def audit_prompt_identity(manifest: dict) -> dict:
    identity = manifest["bundled_prompt_identity"]
    current = ROOT / identity["path"]
    require(current.is_file() and not current.is_symlink(), "bundled_prompt_missing")
    require(sha256_file(current) == identity["sha256"], "bundled_prompt_sha_mismatch")
    prompt_text = current.read_text(encoding="utf-8")
    for required in (
        "你是 CodeWhale",
        "### 事实优先",
        "### 执行闭环",
        "### 验证后完成",
        "### 有收益才使用多 Agent",
    ):
        require(required in prompt_text, f"bundled_prompt_fragment_missing:{required}")

    prompts_source = read_text("crates/context/src/prompts.rs")
    require(
        'pub const BASE_PROMPT: &str = include_str!("prompts/constitution.md");'
        in prompts_source,
        "compile_time_prompt_owner_missing",
    )
    require(
        "CODEWHALE_ALLOW_BASE_PROMPT_OVERRIDE" in prompts_source
        and "if !base_prompt_override_opt_in()" in prompts_source,
        "explicit_override_boundary_missing",
    )

    revisions = {}
    for item in identity["exact_revision_chain"]:
        digest = sha256_bytes(git_show_bytes(item["revision"], identity["path"]))
        require(
            digest == item["constitution_sha256"] == identity["sha256"],
            f"historical_prompt_sha_mismatch:{item['revision']}",
        )
        revisions[item["role"]] = item["revision"]

    historical = identity["historical_prompt_ab"]
    prompt_ab = ROOT / historical["summary_path"]
    convergence = ROOT / historical["convergence_summary_path"]
    require(
        sha256_file(prompt_ab) == historical["summary_sha256"],
        "historical_prompt_ab_summary_mismatch",
    )
    require(
        sha256_file(convergence) == historical["convergence_summary_sha256"],
        "historical_prompt_convergence_summary_mismatch",
    )
    prompt_ab_text = prompt_ab.read_text(encoding="utf-8")
    convergence_text = convergence.read_text(encoding="utf-8")
    require(
        "状态：候选拒绝；不是能力提升证据" in prompt_ab_text
        and "当前候选同时违反前两项，结论为 **拒绝**" in prompt_ab_text,
        "historical_negative_result_missing",
    )
    require(
        "v2、v3 均拒绝" in convergence_text
        and "不继续增加" in convergence_text,
        "historical_convergence_rejection_missing",
    )
    return {
        "path": identity["path"],
        "sha256": identity["sha256"],
        "compile_time_owner": True,
        "explicit_user_override_only": True,
        "exact_revision_roles": revisions,
        "historical_candidate_reclassified_as_win": False,
    }


def audit_m5_evidence(manifest: dict) -> dict:
    evidence = manifest["qualified_real_model_evidence"]
    summary = ROOT / evidence["summary_path"]
    raw = ROOT / evidence["raw_path"]
    require(sha256_file(summary) == evidence["summary_sha256"], "m5_summary_sha_mismatch")
    require(raw.is_file() and not raw.is_symlink(), "m5_raw_missing")
    require(stat.S_IMODE(raw.stat().st_mode) == 0o600, "m5_raw_mode_mismatch")
    require(sha256_file(raw) == evidence["raw_sha256"], "m5_raw_sha_mismatch")
    result = json.loads(raw.read_bytes())
    require(result.get("evaluation_id") == evidence["evaluation_id"], "m5_id_mismatch")
    require(len(result.get("records", [])) == 12, "m5_arm_count_mismatch")
    aggregate = result.get("aggregate", {})
    require(aggregate.get("product_metric_eligible") is True, "m5_not_metric_eligible")
    require(
        aggregate.get("pair_first_model_request_equal") is True
        and aggregate.get("exact_cell_repetitions") is True,
        "m5_pair_identity_invalid",
    )
    cells = {
        (cell["scenario"], cell["variant"]): cell
        for cell in aggregate.get("cells", [])
    }
    require(
        cells[("coding_fix", "baseline")]["verified_success"] == 3
        and cells[("coding_fix", "candidate")]["verified_success"] == 3
        and cells[("coding_fix", "candidate")]["false_success"] == 0,
        "m5_coding_result_mismatch",
    )
    require(
        cells[("forced_false_claim", "baseline")]["false_success"] == 3
        and cells[("forced_false_claim", "candidate")]["correct_rejection"] == 3
        and cells[("forced_false_claim", "candidate")]["false_success"] == 0,
        "m5_false_success_result_mismatch",
    )
    require(
        all(
            record.get("measurement_valid") is True
            and record.get("contract_valid") is True
            and record.get("completion_gate_valid") is True
            and record.get("accounting", {}).get("valid") is True
            for record in result["records"]
        ),
        "m5_arm_validity_mismatch",
    )
    prompt_sha = manifest["bundled_prompt_identity"]["sha256"]
    for revision in (
        result["baseline"]["source_revision"],
        result["candidate"]["source_revision"],
    ):
        require(
            sha256_bytes(git_show_bytes(revision, PROMPT_PATH)) == prompt_sha,
            f"m5_prompt_identity_mismatch:{revision}",
        )
    return {
        "evaluation_id": result["evaluation_id"],
        "arms": 12,
        "product_metric_eligible": True,
        "coding_verified": {"baseline": 3, "candidate": 3},
        "candidate_false_success": 0,
        "candidate_correct_rejections": 3,
        "api_requests": evidence["aggregate"]["api_requests"],
        "tokens": evidence["aggregate"]["tokens"],
        "cost_usd": evidence["aggregate"]["cost_usd"],
        "prompt_sha256": prompt_sha,
        "raw_sha256": evidence["raw_sha256"],
        "raw_mode": "0600",
    }


def audit_m8m_diagnostic(manifest: dict) -> dict:
    evidence = manifest["current_live_diagnostic"]
    summary = ROOT / evidence["summary_path"]
    raw = ROOT / evidence["raw_path"]
    require(sha256_file(summary) == evidence["summary_sha256"], "m8m_summary_sha_mismatch")
    require(raw.is_file() and not raw.is_symlink(), "m8m_raw_missing")
    require(stat.S_IMODE(raw.stat().st_mode) == 0o600, "m8m_raw_mode_mismatch")
    require(sha256_file(raw) == evidence["raw_sha256"], "m8m_raw_sha_mismatch")
    observations = []
    last_payload = None
    with raw.open("r", encoding="utf-8") as handle:
        for line in handle:
            record = json.loads(line)
            payload = record["payload"]
            last_payload = payload
            if payload.get("record_type") == "arm_observation":
                projection = payload["projection"]
                observations.append(
                    {
                        "task_id": projection["task_id"],
                        "variant": projection["variant"],
                        "verified_success": projection["verified_success"],
                        "false_success": projection["false_success"],
                        "measurement_valid": projection["measurement_valid"],
                        "billing_unknown": projection["accounting"]["billing_unknown"],
                    }
                )
    require(len(observations) == 6, "m8m_observation_count_mismatch")
    baseline = {
        item["task_id"]: item
        for item in observations
        if item["variant"] == "baseline"
    }
    require(set(baseline) == {"t1", "t3", "t5"}, "m8m_baseline_tasks_mismatch")
    require(
        all(
            item["verified_success"] is True
            and item["false_success"] is False
            and item["measurement_valid"] is True
            and item["billing_unknown"] is False
            for item in baseline.values()
        ),
        "m8m_baseline_diagnostic_invalid",
    )
    require(
        last_payload is not None
        and last_payload.get("record_type") == "suite_abort"
        and last_payload.get("code") == "aborted_unknown_billing",
        "m8m_abort_identity_mismatch",
    )
    return {
        "baseline_tasks": sorted(baseline),
        "baseline_verified": 3,
        "baseline_false_success": 0,
        "product_metric_eligible": False,
        "candidate_benefit_claim_allowed": False,
        "suite_abort": "aborted_unknown_billing",
        "raw_sha256": evidence["raw_sha256"],
        "raw_mode": "0600",
    }


def audit_writer_evidence(manifest: dict) -> dict:
    evidence = manifest["writer_evidence"]
    summary = ROOT / evidence["summary_path"]
    raw = ROOT / evidence["raw_path"]
    require(sha256_file(summary) == evidence["summary_sha256"], "writer_summary_sha_mismatch")
    require(raw.is_file() and not raw.is_symlink(), "writer_raw_missing")
    require(stat.S_IMODE(raw.stat().st_mode) == 0o600, "writer_raw_mode_mismatch")
    require(sha256_file(raw) == evidence["raw_sha256"], "writer_raw_sha_mismatch")
    require(
        sha256_bytes(git_show_bytes(evidence["revision"], PROMPT_PATH))
        == evidence["constitution_sha256"],
        "writer_prompt_identity_mismatch",
    )
    result = json.loads(raw.read_bytes())
    require(result.get("status") == "passed", "writer_canary_not_passed")
    return {
        "revision": evidence["revision"],
        "status": "passed",
        "product_metric_eligible": False,
        "prompt_sha256": evidence["constitution_sha256"],
        "raw_sha256": evidence["raw_sha256"],
        "raw_mode": "0600",
    }


def audit_release_owner(manifest: dict) -> dict:
    evidence = manifest["release_identity_and_rollback"]
    owner = ROOT / evidence["owner"]
    summary = ROOT / evidence["historical_summary_path"]
    require(
        sha256_bytes(git_show_bytes(manifest["baseline"]["revision"], evidence["owner"]))
        == evidence["owner_sha256_at_baseline"],
        "baseline_delivery_owner_sha_mismatch",
    )
    require(
        sha256_file(summary) == evidence["historical_summary_sha256"],
        "delivery_summary_sha_mismatch",
    )
    source = owner.read_text(encoding="utf-8")
    for required in (
        "source_revision",
        "source_tree",
        "cargo_lock_sha256",
        "atomic_symlink",
        "rollback_command",
        "verify_internal_checksums",
    ):
        require(required in source, f"delivery_contract_missing:{required}")
    for forbidden in ("curl ", "wget ", "git fetch", "git pull", "gh release"):
        require(forbidden not in source, f"delivery_network_command_present:{forbidden}")
    return {
        "owner": evidence["owner"],
        "sha256": sha256_file(owner),
        "source_identity": True,
        "atomic_current_previous": True,
        "network_release_discovery": False,
    }


def audit_authority() -> dict:
    adr = read_text("docs/decisions/0007-v1-fixed-chinese-prompt-release-evidence.md")
    product = read_text("docs/product/PRODUCT_PLAN.md")
    require("- 状态：已接受" in adr, "adr_0007_not_accepted")
    require("未接管的候选不构成发布依赖" in adr, "adr_candidate_boundary_missing")
    require("不改写为中文 prompt 优于英文 prompt" in adr, "adr_non_conclusion_missing")
    v1 = product.split("## 14. V1 完成定义", 1)[1].split("## 15.", 1)[0]
    require(
        "固定中文 Agent prompt 具有可追溯、可回滚的版本身份" in v1,
        "product_v15_successor_missing",
    )
    require(
        "任何新的模型可见提示语义候选" in v1
        and "同任务 A/B" in v1,
        "future_candidate_ab_gate_missing",
    )
    require(
        "中文原生 Agent 提示词通过同任务 A/B" not in v1,
        "old_absolute_v15_still_present",
    )
    for path in (
        "docs/product/ROADMAP.md",
        "docs/product/EVALUATION.md",
        "docs/architecture/CURRENT_CODEWHALE.md",
    ):
        authority = read_text(path)
        require("16 pass / 0 blocked" in authority, f"matrix_not_closed:{path}")
        require("V1 可发布" in authority, f"release_decision_missing:{path}")
        require(
            "inadmissible_no_material_treatment" in authority,
            f"no_live_admission_missing:{path}",
        )
    require(
        not (ROOT / "eval/fixtures/m8-d-prompt/v1/constitution.md").exists(),
        "m8d_candidate_fixture_not_deleted",
    )
    production = read_text("crates/app/src/production.rs")
    require(
        "m8d_prompt_treatment" not in production
        and "CODEWHALE_M8D_PROBE" not in production,
        "m8d_candidate_app_test_not_deleted",
    )
    return {
        "adr_0007": "accepted",
        "v1_matrix": "16 pass / 0 blocked",
        "release_status": "releasable",
        "future_candidate_ab_gate": True,
        "m8d_candidate_path_deleted": True,
    }


def git_identity() -> dict:
    return {
        "branch": git("branch", "--show-current"),
        "revision": git("rev-parse", "HEAD"),
        "tree": git("rev-parse", "HEAD^{tree}"),
        "dirty": bool(git("status", "--porcelain=v1", "--untracked-files=all")),
    }


def run_gate(gate: dict) -> dict:
    environment = os.environ.copy()
    environment["CARGO_INCREMENTAL"] = "0"
    environment["CARGO_TARGET_DIR"] = TARGET_DIR
    environment["CARGO_NET_OFFLINE"] = "true"
    command = ["cargo", "test", "-p", gate["package"], "--locked"]
    if gate.get("test_target"):
        command.extend(["--test", gate["test_target"]])
    command.extend([gate["filter"], "--", "--nocapture"])
    started = time.monotonic()
    completed = subprocess.run(
        command,
        cwd=ROOT,
        env=environment,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    result = {
        "id": gate["id"],
        "package": gate["package"],
        "test_target": gate.get("test_target"),
        "filter": gate["filter"],
        "exit_code": completed.returncode,
        "wall_time_ms": round((time.monotonic() - started) * 1000),
    }
    if completed.returncode != 0:
        result["diagnostic_tail"] = completed.stdout[-65_536:]
    return result


def parse_manifest(path: Path) -> dict:
    values = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        key, value = line.split("\t", 1)
        values[key] = value
    return values


def run_release_lifecycle(artifact: Path, candidate: dict) -> dict:
    artifact = artifact.resolve()
    require(artifact.is_file() and not artifact.is_symlink(), "release_artifact_missing")
    checksum = Path(f"{artifact}.sha256")
    require(checksum.is_file() and not checksum.is_symlink(), "release_checksum_missing")
    delivery = ROOT / "scripts/codewhale-delivery.sh"
    with tempfile.TemporaryDirectory(prefix="codewhale-m8n-release-") as raw:
        root = Path(raw)
        prefix = root / "prefix"
        home = root / "home"
        home.mkdir()
        sentinel = home / "sentinel"
        sentinel.write_text("preserve-m8n\n", encoding="utf-8")
        sentinel_sha = sha256_file(sentinel)
        environment = os.environ.copy()
        environment["CODEWHALE_HOME"] = str(home)

        def delivery_call(*arguments: str) -> subprocess.CompletedProcess:
            return subprocess.run(
                [str(delivery), *arguments],
                cwd=ROOT,
                env=environment,
                check=True,
                text=True,
                capture_output=True,
            )

        delivery_call("install", "--artifact", str(artifact), "--prefix", str(prefix))
        delivery_call("verify", "--prefix", str(prefix))
        current = (prefix / "lib/codewhale/current").resolve(strict=True)
        installed_manifest = parse_manifest(current / "manifest.tsv")
        require(
            installed_manifest.get("source_revision") == candidate["revision"],
            "artifact_revision_mismatch",
        )
        require(
            installed_manifest.get("source_tree") == candidate["tree"],
            "artifact_tree_mismatch",
        )
        require(
            installed_manifest.get("cargo_lock_sha256")
            == sha256_file(ROOT / "Cargo.lock"),
            "artifact_cargo_lock_mismatch",
        )
        codewhale_version = subprocess.run(
            [str(prefix / "bin/codewhale"), "--version"],
            check=True,
            text=True,
            capture_output=True,
        ).stdout.strip()
        tui_version = subprocess.run(
            [str(prefix / "bin/codewhale-tui"), "--version"],
            check=True,
            text=True,
            capture_output=True,
        ).stdout.strip()
        require(
            candidate["revision"][:12] in codewhale_version
            and candidate["revision"][:12] in tui_version,
            "installed_binary_revision_mismatch",
        )
        binary_hashes = {
            "codewhale": sha256_file(current / "bin/codewhale"),
            "codewhale_tui": sha256_file(current / "bin/codewhale-tui"),
        }
        delivery_call("uninstall", "--prefix", str(prefix))
        require(
            sentinel.is_file() and sha256_file(sentinel) == sentinel_sha,
            "uninstall_changed_user_data",
        )
        require(
            not (prefix / "lib/codewhale").exists()
            and not (prefix / "bin/codewhale").exists()
            and not (prefix / "bin/codewhale-tui").exists(),
            "uninstall_left_program_state",
        )
    return {
        "artifact": artifact.name,
        "artifact_sha256": sha256_file(artifact),
        "checksum_sha256": sha256_file(checksum),
        "manifest": installed_manifest,
        "binary_hashes": binary_hashes,
        "codewhale_version": codewhale_version,
        "codewhale_tui_version": tui_version,
        "install_verify_uninstall": "passed",
        "user_data_preserved": True,
    }


def write_private_json(path: Path, value: dict) -> None:
    path = path.resolve()
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    if path.exists() or path.is_symlink():
        raise HarnessError("output_must_not_exist")
    descriptor, temporary = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    try:
        os.fchmod(descriptor, stat.S_IRUSR | stat.S_IWUSR)
        with os.fdopen(descriptor, "wb", closefd=True) as handle:
            handle.write(canonical_json(value))
            handle.write(b"\n")
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, path)
        require(stat.S_IMODE(path.stat().st_mode) == 0o600, "output_mode_mismatch")
    except BaseException:
        try:
            os.unlink(temporary)
        except FileNotFoundError:
            pass
        raise


def self_test() -> int:
    manifest, manifest_sha = load_manifest()
    prompt = audit_prompt_identity(manifest)
    m5 = audit_m5_evidence(manifest)
    m8m = audit_m8m_diagnostic(manifest)
    writer = audit_writer_evidence(manifest)
    release = audit_release_owner(manifest)
    with tempfile.TemporaryDirectory(prefix="codewhale-m8n-self-test-") as raw:
        output = Path(raw) / "result.json"
        value = {"schema": EXPECTED_SCHEMA, "manifest_sha256": manifest_sha}
        write_private_json(output, value)
        require(json.loads(output.read_bytes()) == value, "private_output_round_trip_failed")
        try:
            write_private_json(output, value)
        except HarnessError as error:
            require(str(error) == "output_must_not_exist", "wrong_overwrite_error")
        else:
            raise HarnessError("existing_output_did_not_fail")
    print(
        json.dumps(
            {
                "status": "pass",
                "manifest_sha256": manifest_sha,
                "prompt_sha256": prompt["sha256"],
                "qualified_real_model_arms": m5["arms"],
                "current_baseline_diagnostic_tasks": len(m8m["baseline_tasks"]),
                "writer_canary": writer["status"],
                "release_owner": release["owner"],
                "material_model_visible_delta": False,
                "credential_read": False,
                "official_api_requests": 0,
                "network_accessed": False,
            },
            ensure_ascii=False,
            sort_keys=True,
        )
    )
    return 0


def run(candidate_revision: str, artifact: Path, output: Path) -> int:
    manifest, manifest_sha = load_manifest()
    before = git_identity()
    require(not before["dirty"], "formal_run_requires_clean_tree")
    require(before["branch"] == manifest["baseline"]["branch"], "branch_mismatch")
    require(before["revision"] == candidate_revision, "candidate_revision_mismatch")
    require(
        git("merge-base", "--is-ancestor", manifest["baseline"]["revision"], candidate_revision)
        == "",
        "candidate_not_descendant_of_baseline",
    )
    prompt = audit_prompt_identity(manifest)
    m5 = audit_m5_evidence(manifest)
    m8m = audit_m8m_diagnostic(manifest)
    writer = audit_writer_evidence(manifest)
    release_owner = audit_release_owner(manifest)
    authority = audit_authority()
    gates = [run_gate(gate) for gate in manifest["current_release_gates"]]
    release = run_release_lifecycle(artifact, before)
    after = git_identity()
    passed = (
        before == after
        and all(gate["exit_code"] == 0 for gate in gates)
        and m5["product_metric_eligible"]
        and m8m["candidate_benefit_claim_allowed"] is False
        and authority["v1_matrix"] == "16 pass / 0 blocked"
        and release["install_verify_uninstall"] == "passed"
    )
    result = {
        "schema": "codewhale.eval.m8-n-v15-release-scope-successor-result.v1",
        "suite_id": manifest["suite_id"],
        "manifest_sha256": manifest_sha,
        "harness_sha256": sha256_file(Path(__file__)),
        "source_before": before,
        "source_after": after,
        "maximum_reruns": 0,
        "prompt_identity": prompt,
        "qualified_real_model_evidence": m5,
        "current_live_diagnostic": m8m,
        "writer_evidence": writer,
        "release_owner": release_owner,
        "authority": authority,
        "gates": gates,
        "release_artifact": release,
        "material_model_visible_delta": False,
        "live_api_admission": "inadmissible_no_material_treatment",
        "credential_read": False,
        "official_api_requests": 0,
        "network_accessed": False,
        "decision": (
            "keep_fixed_chinese_prompt_close_V15_release_V1"
            if passed
            else "offline_v15_release_scope_failed"
        ),
        "v1_matrix": "16 pass / 0 blocked" if passed else "unchanged",
        "release_status": "releasable" if passed else "not_releasable",
        "passed": passed,
    }
    write_private_json(output, result)
    print(
        json.dumps(
            {
                "status": "passed" if passed else "failed",
                "suite_id": result["suite_id"],
                "failed_gates": [
                    gate["id"] for gate in gates if gate["exit_code"] != 0
                ],
                "v1_matrix": result["v1_matrix"],
                "release_status": result["release_status"],
                "credential_read": False,
                "official_api_requests": 0,
                "output": str(output.resolve()),
            },
            ensure_ascii=False,
            sort_keys=True,
        )
    )
    return 0 if passed else 1


def main() -> int:
    parser = argparse.ArgumentParser()
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("self-test")
    run_parser = commands.add_parser("run")
    run_parser.add_argument("--candidate-revision", required=True)
    run_parser.add_argument("--artifact", type=Path, required=True)
    run_parser.add_argument(
        "--output",
        type=Path,
        default=ROOT / "eval/results/m8-n-v15-release-scope-21200ccf-v1.json",
    )
    arguments = parser.parse_args()
    try:
        if arguments.command == "self-test":
            return self_test()
        return run(arguments.candidate_revision, arguments.artifact, arguments.output)
    except (
        HarnessError,
        KeyError,
        OSError,
        subprocess.SubprocessError,
        json.JSONDecodeError,
        UnicodeError,
    ) as error:
        print(f"m8n_harness_error:{error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
