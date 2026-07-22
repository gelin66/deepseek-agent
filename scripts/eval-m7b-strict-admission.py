#!/usr/bin/env python3
"""M7-B Strict treatment admission gate.

This harness is intentionally credential-free. It proves whether the current
production catalogs expose a real Standard/Strict treatment delta before any
release build, key read, or official DeepSeek request is allowed.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_MANIFEST = ROOT / "eval" / "manifests" / "m7-b-strict-admission-v2.json"
EXPECTED_SCHEMA = "codewhale.eval.m7-b-strict-admission.v2"
EXPECTED_RESULT_SCHEMA = "codewhale.eval.m7-b-strict-admission-result.v2"
EXPECTED_EVALUATION_ID = "m7-b-strict-admission-v2"
EXPECTED_DECISION = "inadmissible_no_surface_delta"
EXPECTED_PROTOCOL_VERSIONS = {
    "run_api": 10,
    "runtime_event": 16,
    "state": 21,
    "exec_stream": 2,
}
EXPECTED_ACTORS = {
    "root_headless",
    "root_interactive",
    "coordinator",
    "read_only_child",
    "read_only_depth_limit",
    "isolated_writer",
    "terminal_empty",
}
SHA256_RE = re.compile(r"^sha256:[0-9a-f]{64}$")
REVISION_RE = re.compile(r"^[0-9a-f]{40}$")


class AdmissionError(RuntimeError):
    pass


def sha256_bytes(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    return sha256_bytes(path.read_bytes())


def load_manifest(path: Path) -> dict[str, Any]:
    try:
        manifest = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise AdmissionError(f"manifest_unreadable:{error}") from error
    validate_manifest(manifest)
    return manifest


def validate_manifest(manifest: dict[str, Any]) -> None:
    if manifest.get("schema") != EXPECTED_SCHEMA:
        raise AdmissionError("manifest_schema_mismatch")
    if manifest.get("evaluation_id") != EXPECTED_EVALUATION_ID:
        raise AdmissionError("evaluation_id_mismatch")
    if manifest.get("status") != "frozen_before_any_live_api":
        raise AdmissionError("manifest_not_frozen_before_live_api")
    if manifest.get("protocol_versions") != EXPECTED_PROTOCOL_VERSIONS:
        raise AdmissionError("protocol_versions_mismatch")
    revision = manifest.get("candidate_revision")
    parent = manifest.get("candidate_parent")
    if not isinstance(revision, str) or not REVISION_RE.fullmatch(revision):
        raise AdmissionError("candidate_revision_invalid")
    if not isinstance(parent, str) or not REVISION_RE.fullmatch(parent):
        raise AdmissionError("candidate_parent_invalid")
    contract = manifest.get("decision_contract")
    if not isinstance(contract, dict):
        raise AdmissionError("decision_contract_missing")
    if contract.get("inadmissible_decision") != EXPECTED_DECISION:
        raise AdmissionError("decision_contract_mismatch")
    if contract.get("product_metric_eligible") is not False:
        raise AdmissionError("inadmissible_must_not_be_product_metric_eligible")
    if contract.get("live_api_allowed_when_inadmissible") is not False:
        raise AdmissionError("live_api_must_be_forbidden")
    identity = manifest.get("experiment_identity")
    if not isinstance(identity, dict):
        raise AdmissionError("experiment_identity_missing")
    if identity.get("official_api_request_limit_before_admission") != 0:
        raise AdmissionError("official_api_pre_admission_limit_must_be_zero")
    if identity.get("credential_access_before_admission") != "forbidden":
        raise AdmissionError("credential_access_must_be_forbidden")
    if identity.get("maximum_reruns") != 0:
        raise AdmissionError("maximum_reruns_must_be_zero")

    owners = manifest.get("source_owners")
    if not isinstance(owners, list) or not owners:
        raise AdmissionError("source_owners_missing")
    owner_paths: set[str] = set()
    for owner in owners:
        if not isinstance(owner, dict):
            raise AdmissionError("source_owner_invalid")
        path = owner.get("path")
        digest = owner.get("sha256")
        if not isinstance(path, str) or not path or path in owner_paths:
            raise AdmissionError("source_owner_path_invalid")
        if not isinstance(digest, str) or not SHA256_RE.fullmatch(digest):
            raise AdmissionError("source_owner_sha256_invalid")
        owner_paths.add(path)

    tests = manifest.get("offline_tests")
    if not isinstance(tests, list) or len(tests) != 2:
        raise AdmissionError("offline_test_contract_invalid")
    filters = {test.get("filter") for test in tests if isinstance(test, dict)}
    expected_filters = {
        "actual_actor_catalogs_freeze_strict_fallback_without_touching_historical_manifests",
        "standard_and_strict_candidate_send_the_same_fallback_catalog",
    }
    if filters != expected_filters:
        raise AdmissionError("offline_test_filters_mismatch")

    actors = manifest.get("actor_catalogs")
    if not isinstance(actors, list) or len(actors) != 7:
        raise AdmissionError("actor_catalog_matrix_incomplete")
    actor_names: set[str] = set()
    product_actors = 0
    for actor in actors:
        if not isinstance(actor, dict):
            raise AdmissionError("actor_catalog_invalid")
        name = actor.get("actor")
        digest = actor.get("catalog_sha256")
        if not isinstance(name, str) or name in actor_names:
            raise AdmissionError("actor_name_invalid")
        if not isinstance(digest, str) or not SHA256_RE.fullmatch(digest):
            raise AdmissionError("actor_catalog_sha256_invalid")
        if "strict_issue_count" in actor:
            raise AdmissionError("unverified_strict_issue_count_forbidden")
        surface = actor.get("strict_enabled_surface")
        blocker = actor.get("first_blocker")
        if name == "terminal_empty":
            if blocker is not None or surface != "standard_chat_no_tools":
                raise AdmissionError("terminal_actor_contract_mismatch")
        else:
            product_actors += 1
            if not isinstance(blocker, dict) or any(
                not isinstance(blocker.get(field), str) or not blocker[field]
                for field in ("tool", "path", "code")
            ):
                raise AdmissionError("product_actor_missing_strict_blocker")
            if surface != "standard_chat":
                raise AdmissionError("unexpected_product_actor_surface")
        actor_names.add(name)
    if product_actors != 6 or actor_names != EXPECTED_ACTORS:
        raise AdmissionError("product_actor_count_mismatch")

    expected = manifest.get("expected_result")
    if not isinstance(expected, dict):
        raise AdmissionError("expected_result_missing")
    if expected.get("admitted") is not False:
        raise AdmissionError("expected_admission_must_be_false")
    if expected.get("decision") != EXPECTED_DECISION:
        raise AdmissionError("expected_decision_mismatch")
    if expected.get("official_api_requests") != 0 or expected.get("credential_read") is not False:
        raise AdmissionError("expected_external_exposure_must_be_zero")


def git(*args: str, check: bool = True) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(
        ["git", *args],
        cwd=ROOT,
        check=check,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


def verify_repository(manifest: dict[str, Any]) -> dict[str, Any]:
    candidate = manifest["candidate_revision"]
    parent = git("rev-parse", f"{candidate}^").stdout.decode().strip()
    if parent != manifest["candidate_parent"]:
        raise AdmissionError("candidate_parent_mismatch")
    current = git("rev-parse", "HEAD").stdout.decode().strip()
    if git("merge-base", "--is-ancestor", candidate, current, check=False).returncode != 0:
        raise AdmissionError("candidate_not_ancestor_of_current_head")
    if git("status", "--porcelain").stdout:
        raise AdmissionError("worktree_not_clean")

    owners: list[dict[str, str]] = []
    for owner in manifest["source_owners"]:
        path = owner["path"]
        frozen = git("show", f"{candidate}:{path}").stdout
        frozen_sha256 = sha256_bytes(frozen)
        if frozen_sha256 != owner["sha256"]:
            raise AdmissionError(f"frozen_source_hash_mismatch:{path}")
        owners.append({"path": path, "sha256": frozen_sha256})
    return {"candidate_revision": candidate, "harness_revision": current, "source_owners": owners}


def run_offline_tests(manifest: dict[str, Any], target_dir: Path) -> list[dict[str, Any]]:
    results: list[dict[str, Any]] = []
    environment = os.environ.copy()
    environment["CARGO_INCREMENTAL"] = "0"
    environment["CARGO_TARGET_DIR"] = str(target_dir)
    candidate = manifest["candidate_revision"]
    with tempfile.TemporaryDirectory(prefix="codewhale-m7b-admission-") as directory:
        worktree = Path(directory) / "candidate"
        added = False
        try:
            git("worktree", "add", "--detach", str(worktree), candidate)
            added = True
            for test in manifest["offline_tests"]:
                command = [
                    "cargo",
                    "test",
                    "-p",
                    test["package"],
                    "--locked",
                    test["filter"],
                    "--",
                    "--nocapture",
                ]
                process = subprocess.run(
                    command,
                    cwd=worktree,
                    env=environment,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.STDOUT,
                    text=True,
                )
                output_sha256 = sha256_bytes(process.stdout.encode("utf-8"))
                if process.returncode != 0:
                    raise AdmissionError(
                        f"offline_test_failed:{test['filter']}:{output_sha256}"
                    )
                expected_line = f"test production::tests::{test['filter']} ... ok"
                if "running 1 test" not in process.stdout or expected_line not in process.stdout:
                    raise AdmissionError(
                        f"offline_test_not_executed:{test['filter']}:{output_sha256}"
                    )
                results.append(
                    {
                        "package": test["package"],
                        "filter": test["filter"],
                        "exit_code": process.returncode,
                        "output_sha256": output_sha256,
                    }
                )
        finally:
            if added:
                removed = git("worktree", "remove", "--force", str(worktree), check=False)
                if removed.returncode != 0 and sys.exc_info()[0] is None:
                    raise AdmissionError("candidate_worktree_cleanup_failed")
    return results


def write_exclusive_json(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    payload = (json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n").encode()
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
    descriptor = os.open(path, flags, 0o600)
    try:
        with os.fdopen(descriptor, "wb") as output:
            output.write(payload)
            output.flush()
            os.fsync(output.fileno())
    except BaseException:
        path.unlink(missing_ok=True)
        raise


def preflight(manifest_path: Path, output_path: Path, target_dir: Path) -> dict[str, Any]:
    if output_path.exists():
        raise AdmissionError("output_already_exists")
    manifest = load_manifest(manifest_path)
    repository = verify_repository(manifest)
    test_results = run_offline_tests(manifest, target_dir)
    result = {
        "schema": EXPECTED_RESULT_SCHEMA,
        "evaluation_id": manifest["evaluation_id"],
        "manifest": {
            "path": str(manifest_path.relative_to(ROOT)),
            "sha256": sha256_file(manifest_path),
        },
        "harness": {
            "path": str(Path(__file__).resolve().relative_to(ROOT)),
            "sha256": sha256_file(Path(__file__).resolve()),
        },
        "repository": repository,
        "offline_tests": test_results,
        "actor_catalogs": manifest["actor_catalogs"],
        "admission": {
            "admitted": False,
            "decision": EXPECTED_DECISION,
            "reason": "strict_enabled=true 对所有可执行 production actor 仍选择 Standard Chat；control/treatment 没有真实 surface delta",
            "product_metric_eligible": False,
        },
        "external_exposure": {
            "credential_parameter_supported": False,
            "credential_read": False,
            "official_api_requests": 0,
            "local_loopback_only": True,
        },
        "formal_ab": {
            "started": False,
            "arms": 0,
            "maximum_reruns": 0,
            "binary_freeze_required": False,
            "reason": "treatment admission failed before release build and credential access",
        },
        "decision": {
            "strict_default": "not_admitted",
            "planner": "keep",
            "atomic_standard_fallback": "keep",
            "schema_transformer": "reject",
            "user_strict_toggle": "delete",
        },
    }
    write_exclusive_json(output_path, result)
    return result


class HarnessTests(unittest.TestCase):
    def test_frozen_manifest_is_valid(self) -> None:
        validate_manifest(json.loads(DEFAULT_MANIFEST.read_text(encoding="utf-8")))

    def test_live_or_credential_exposure_is_rejected(self) -> None:
        manifest = json.loads(DEFAULT_MANIFEST.read_text(encoding="utf-8"))
        manifest["experiment_identity"]["official_api_request_limit_before_admission"] = 1
        with self.assertRaisesRegex(AdmissionError, "official_api_pre_admission"):
            validate_manifest(manifest)

    def test_missing_actor_blocker_is_rejected(self) -> None:
        manifest = json.loads(DEFAULT_MANIFEST.read_text(encoding="utf-8"))
        manifest["actor_catalogs"][0]["first_blocker"] = None
        with self.assertRaisesRegex(AdmissionError, "product_actor_missing_strict_blocker"):
            validate_manifest(manifest)

    def test_unverified_issue_count_is_rejected(self) -> None:
        manifest = json.loads(DEFAULT_MANIFEST.read_text(encoding="utf-8"))
        manifest["actor_catalogs"][0]["strict_issue_count"] = 1
        with self.assertRaisesRegex(AdmissionError, "unverified_strict_issue_count_forbidden"):
            validate_manifest(manifest)

    def test_output_is_exclusive_and_private(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "result.json"
            write_exclusive_json(path, {"ok": True})
            self.assertEqual(path.stat().st_mode & 0o777, 0o600)
            with self.assertRaises(FileExistsError):
                write_exclusive_json(path, {"ok": False})


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    subcommands = parser.add_subparsers(dest="command", required=True)
    subcommands.add_parser("self-test", help="run credential-free harness tests")
    preflight_parser = subcommands.add_parser(
        "preflight", help="run the frozen offline treatment admission gate"
    )
    preflight_parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    preflight_parser.add_argument("--output", type=Path, required=True)
    preflight_parser.add_argument(
        "--target-dir", type=Path, default=Path("/private/tmp/codewhale-m7b-target")
    )
    return parser.parse_args()


def main() -> int:
    arguments = parse_args()
    if arguments.command == "self-test":
        suite = unittest.defaultTestLoader.loadTestsFromTestCase(HarnessTests)
        result = unittest.TextTestRunner(verbosity=2).run(suite)
        return 0 if result.wasSuccessful() else 1
    try:
        result = preflight(
            arguments.manifest.resolve(), arguments.output.resolve(), arguments.target_dir.resolve()
        )
    except (AdmissionError, OSError, subprocess.SubprocessError) as error:
        print(f"M7-B Strict 准入失败：{error}", file=sys.stderr)
        return 2
    print(json.dumps(result["admission"], ensure_ascii=False, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
