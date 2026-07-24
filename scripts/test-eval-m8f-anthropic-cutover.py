#!/usr/bin/env python3
"""Offline identity and contract tests for the frozen M8-F cutover."""

from __future__ import annotations

import hashlib
import json
import pathlib
import subprocess
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "eval/manifests/m8-f-anthropic-messages-cutover-v1.json"


def sha256_at_revision(revision: str, path: str) -> str:
    content = subprocess.run(
        ["git", "show", f"{revision}:{path}"],
        cwd=ROOT,
        check=True,
        capture_output=True,
    ).stdout
    return hashlib.sha256(content).hexdigest()


class M8FAnthropicMessagesContract(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
        cls.revision = cls.manifest["control_revision"]

    def test_manifest_identity_and_execution_are_closed(self) -> None:
        self.assertEqual(
            self.manifest["schema"],
            "codewhale.eval.m8-f-anthropic-messages-cutover.v1",
        )
        self.assertEqual(self.manifest["control_tree"], "3f2225698faaf9a72f9beea3e8b9d6c9ee4e1425")
        self.assertEqual(self.manifest["execution"]["maximum_reruns"], 0)
        self.assertFalse(
            self.manifest["execution"]["credential_read_before_offline_gates"]
        )
        self.assertFalse(
            self.manifest["execution"]["official_api_requests_before_offline_gates"]
        )
        self.assertFalse(self.manifest["execution"]["overwrite"])

    def test_authorities_bind_the_frozen_control(self) -> None:
        expected = self.manifest["authorities"]
        for key, path in {
            "product_plan_sha256": "docs/product/PRODUCT_PLAN.md",
            "roadmap_sha256": "docs/product/ROADMAP.md",
            "evaluation_sha256": "docs/product/EVALUATION.md",
            "current_architecture_sha256": "docs/architecture/CURRENT_CODEWHALE.md",
        }.items():
            self.assertEqual(expected[key], sha256_at_revision(self.revision, path))

    def test_control_blobs_bind_the_old_callable_path(self) -> None:
        expected = self.manifest["control_blobs"]
        for key, path in {
            "deepseek_planner_sha256": "crates/deepseek/src/lib.rs",
            "deepseek_transport_sha256": "crates/deepseek/src/transport.rs",
            "deepseek_accounting_sha256": "crates/deepseek/src/accounting.rs",
            "deepseek_model_port_sha256": "crates/deepseek/src/model_port.rs",
            "deepseek_config_sha256": "crates/config/src/deepseek.rs",
            "production_application_sha256": "crates/app/src/production.rs",
        }.items():
            self.assertEqual(expected[key], sha256_at_revision(self.revision, path))

        planner = subprocess.run(
            ["git", "show", f"{self.revision}:crates/deepseek/src/lib.rs"],
            cwd=ROOT,
            check=True,
            capture_output=True,
            text=True,
        ).stdout
        transport = subprocess.run(
            ["git", "show", f"{self.revision}:crates/deepseek/src/transport.rs"],
            cwd=ROOT,
            check=True,
            capture_output=True,
            text=True,
        ).stdout
        self.assertIn("/chat/completions", planner)
        self.assertIn(".bearer_auth(", transport)
        self.assertIn("parse_chat_response", transport)

    def test_official_messages_target_is_exact(self) -> None:
        contract = self.manifest["official_contract"]
        self.assertEqual(contract["base_url"], "https://api.deepseek.com/anthropic")
        self.assertEqual(contract["request_path"], "/v1/messages")
        self.assertEqual(
            contract["request_url"],
            "https://api.deepseek.com/anthropic/v1/messages",
        )
        self.assertEqual(contract["authentication_header"], "x-api-key")
        self.assertEqual(contract["stream_terminal"], "message_stop")
        self.assertEqual(
            contract["compatibility_header"],
            {
                "name": "anthropic-version",
                "value": "2023-06-01",
                "deepseek_behavior": "ignored",
            },
        )

    def test_parity_matrix_is_complete_and_unique(self) -> None:
        parity = self.manifest["parity_contract"]
        self.assertEqual(
            [row["id"] for row in parity],
            [f"P{index:02d}" for index in range(1, 13)],
        )
        self.assertEqual(len({row["area"] for row in parity}), len(parity))
        required = {
            "endpoint_and_auth",
            "system_and_messages",
            "tools",
            "thinking_replay",
            "stream_content_blocks",
            "finish_reason",
            "usage_cache_billing",
            "retry_and_side_effect",
            "crash_and_reopen",
            "actor_and_entrypoint_parity",
            "cutover_deletion",
            "official_canary",
        }
        self.assertEqual({row["area"] for row in parity}, required)

    def test_cutover_is_atomic_and_has_no_chat_fallback(self) -> None:
        decision = self.manifest["decision"]
        self.assertEqual(decision["pre_cutover"], "not_admitted")
        self.assertEqual(decision["fail_closed"], "hold_release_and_keep_evidence")
        self.assertIn("permanent dual transport", decision["forbidden"])
        self.assertIn("Chat fallback", decision["forbidden"])
        self.assertIn("second AgentRuntime", decision["forbidden"])
        self.assertIn("second RunStore", decision["forbidden"])


if __name__ == "__main__":
    unittest.main()
