#!/usr/bin/env python3
"""Offline contract tests for the M8-N V15 release-scope successor."""

from __future__ import annotations

import hashlib
import importlib.util
import json
from pathlib import Path
import stat
import subprocess
import unittest


ROOT = Path(__file__).resolve().parents[1]
HARNESS_PATH = ROOT / "scripts/eval-m8n-v15-release-scope.py"
MANIFEST_PATH = ROOT / "eval/manifests/m8-n-v15-release-scope-successor-v1.json"
SPEC = importlib.util.spec_from_file_location("m8n_harness", HARNESS_PATH)
assert SPEC is not None and SPEC.loader is not None
M8N = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(M8N)


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


class M8NV15ReleaseScopeContract(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.manifest = json.loads(MANIFEST_PATH.read_text(encoding="utf-8"))

    def test_frozen_baseline_and_no_live_admission(self) -> None:
        self.assertEqual(
            self.manifest["schema"],
            "codewhale.eval.m8-n-v15-release-scope-successor.v1",
        )
        self.assertEqual(
            self.manifest["baseline"]["revision"],
            "21200ccf0b4118793f1ad4e64acc973c25741e08",
        )
        self.assertEqual(
            self.manifest["baseline"]["v1_matrix"],
            "15 pass / 1 blocked",
        )
        decision = self.manifest["successor_decision_contract"]
        self.assertFalse(decision["material_model_visible_delta"])
        self.assertEqual(
            decision["live_api_admission"],
            "inadmissible_no_material_treatment",
        )
        execution = self.manifest["execution"]
        self.assertEqual(execution["maximum_reruns"], 0)
        self.assertFalse(execution["credential_read"])
        self.assertFalse(execution["official_api_requests_allowed"])
        self.assertFalse(execution["external_network_allowed"])

    def test_product_candidate_gate_is_not_baseline_superiority_claim(self) -> None:
        audit = self.manifest["authority_audit"]
        self.assertTrue(
            audit["product_plan_candidate_gate"][
                "applies_to_unadopted_m8_d_m8_m_candidate"
            ]
        )
        self.assertFalse(
            audit["product_plan_candidate_gate"][
                "requires_candidate_cutover_without_benefit"
            ]
        )
        self.assertIn(
            "future semantic prompt candidate",
            audit["successor_requirement"],
        )
        self.assertIn(
            "before cutover",
            audit["successor_requirement"],
        )

    def test_bundled_prompt_is_byte_identical_across_frozen_evidence(self) -> None:
        identity = self.manifest["bundled_prompt_identity"]
        current = ROOT / identity["path"]
        self.assertEqual(sha256(current), identity["sha256"])
        for item in identity["exact_revision_chain"]:
            raw = subprocess.run(
                ["git", "show", f"{item['revision']}:{identity['path']}"],
                cwd=ROOT,
                check=True,
                capture_output=True,
            ).stdout
            self.assertEqual(hashlib.sha256(raw).hexdigest(), identity["sha256"])

    def test_historical_prompt_treatment_stays_rejected(self) -> None:
        history = self.manifest["bundled_prompt_identity"]["historical_prompt_ab"]
        summary = ROOT / history["summary_path"]
        convergence = ROOT / history["convergence_summary_path"]
        self.assertEqual(sha256(summary), history["summary_sha256"])
        self.assertEqual(sha256(convergence), history["convergence_summary_sha256"])
        self.assertIn(
            "状态：候选拒绝；不是能力提升证据",
            summary.read_text(encoding="utf-8"),
        )
        self.assertIn(
            "v2、v3 均拒绝",
            convergence.read_text(encoding="utf-8"),
        )

    def test_m5_qualified_evidence_is_private_complete_and_same_prompt(self) -> None:
        evidence = self.manifest["qualified_real_model_evidence"]
        raw = ROOT / evidence["raw_path"]
        self.assertEqual(stat.S_IMODE(raw.stat().st_mode), 0o600)
        self.assertEqual(sha256(raw), evidence["raw_sha256"])
        result = json.loads(raw.read_text(encoding="utf-8"))
        self.assertEqual(len(result["records"]), 12)
        self.assertTrue(result["aggregate"]["product_metric_eligible"])
        self.assertTrue(result["aggregate"]["pair_first_model_request_equal"])
        prompt_sha = self.manifest["bundled_prompt_identity"]["sha256"]
        for revision in (
            result["baseline"]["source_revision"],
            result["candidate"]["source_revision"],
        ):
            raw_prompt = subprocess.run(
                ["git", "show", f"{revision}:crates/context/src/prompts/constitution.md"],
                cwd=ROOT,
                check=True,
                capture_output=True,
            ).stdout
            self.assertEqual(hashlib.sha256(raw_prompt).hexdigest(), prompt_sha)

    def test_m8m_is_current_baseline_diagnostic_not_candidate_metric(self) -> None:
        evidence = self.manifest["current_live_diagnostic"]
        raw = ROOT / evidence["raw_path"]
        self.assertEqual(stat.S_IMODE(raw.stat().st_mode), 0o600)
        self.assertEqual(sha256(raw), evidence["raw_sha256"])
        self.assertFalse(evidence["product_metric_eligible"])
        self.assertFalse(evidence["eligible_for_candidate_benefit"])
        audit = M8N.audit_m8m_diagnostic(self.manifest)
        self.assertEqual(audit["baseline_tasks"], ["t1", "t3", "t5"])
        self.assertEqual(audit["baseline_verified"], 3)
        self.assertEqual(audit["suite_abort"], "aborted_unknown_billing")

    def test_writer_evidence_uses_the_same_prompt_and_is_not_benefit_claim(self) -> None:
        evidence = self.manifest["writer_evidence"]
        raw = ROOT / evidence["raw_path"]
        self.assertEqual(stat.S_IMODE(raw.stat().st_mode), 0o600)
        self.assertEqual(sha256(raw), evidence["raw_sha256"])
        result = json.loads(raw.read_text(encoding="utf-8"))
        self.assertEqual(result["status"], "passed")
        self.assertFalse(result["product_metric_eligible"])
        self.assertEqual(
            evidence["constitution_sha256"],
            self.manifest["bundled_prompt_identity"]["sha256"],
        )

    def test_exact_current_gates_cover_prompt_actors_verifier_and_writer(self) -> None:
        gates = self.manifest["current_release_gates"]
        self.assertEqual(
            [gate["id"] for gate in gates],
            ["P01", "P02", "P03", "P04", "P05", "P06", "P07"],
        )
        joined = " ".join(gate["proves"] for gate in gates)
        for required in (
            "prompt",
            "root",
            "read-only",
            "Writer",
            "verifier",
            "accounting",
            "SQLite",
        ):
            self.assertIn(required, joined)

    def test_release_owner_binds_source_and_atomic_rollback(self) -> None:
        evidence = self.manifest["release_identity_and_rollback"]
        owner = ROOT / evidence["owner"]
        source = owner.read_text(encoding="utf-8")
        for required in (
            "source_revision",
            "source_tree",
            "cargo_lock_sha256",
            "atomic_symlink",
            "rollback_command",
        ):
            self.assertIn(required, source)
        self.assertNotIn("curl ", source)
        self.assertNotIn("wget ", source)

    def test_cutover_deletes_candidate_only_paths_not_production_owner(self) -> None:
        deletion = "\n".join(self.manifest["slice_contract"]["cutover_deletion"])
        self.assertIn("M8-D candidate-only", deletion)
        self.assertIn("one-shot M8-N evaluator", deletion)
        self.assertIn("do not add a prompt selector", deletion)
        self.assertEqual(
            self.manifest["slice_contract"]["single_owner"]["production_prompt"],
            "crates/context",
        )

    def test_harness_has_no_credential_or_official_request_path(self) -> None:
        source = HARNESS_PATH.read_text(encoding="utf-8")
        for forbidden in (
            "key.txt",
            "read_key",
            "api.deepseek.com",
            "Authorization",
            "urllib",
            "requests.",
        ):
            self.assertNotIn(forbidden, source)
        self.assertIn('"official_api_requests": 0', source)
        self.assertIn('"credential_read": False', source)


if __name__ == "__main__":
    unittest.main()
