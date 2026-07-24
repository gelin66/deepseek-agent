#!/usr/bin/env python3
"""Offline contract for the M8-K evidence-gated V1 scope successor."""

from __future__ import annotations

import hashlib
import json
import pathlib
import re
import subprocess
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
MANIFEST_PATH = ROOT / "eval/manifests/m8-k-v1-scope-successor-v1.json"


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def read_at_revision(revision: str, path: str) -> bytes:
    return subprocess.run(
        ["git", "show", f"{revision}:{path}"],
        cwd=ROOT,
        check=True,
        capture_output=True,
    ).stdout


def rust_sources(*roots: str) -> str:
    paths = [
        path
        for root in roots
        for path in (ROOT / root).rglob("*.rs")
    ]
    return "\n".join(path.read_text(encoding="utf-8") for path in paths)


class M8KV1ScopeContract(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.manifest = json.loads(MANIFEST_PATH.read_text(encoding="utf-8"))

    def test_frozen_identity_and_no_live_admission(self) -> None:
        self.assertEqual(
            self.manifest["schema"],
            "codewhale.eval.m8-k-v1-scope-successor.v1",
        )
        self.assertEqual(
            self.manifest["baseline"]["revision"],
            "49a465811e96036bc985a23fcd4a69a6391861fe",
        )
        self.assertEqual(
            self.manifest["selection_gate"]["selected_path"],
            "product_scope_successor_adr",
        )
        self.assertIsNone(
            self.manifest["selection_gate"]["selected_implementation_blocker"]
        )
        self.assertFalse(self.manifest["execution"]["credential_read"])
        self.assertFalse(
            self.manifest["execution"]["official_api_requests_allowed"]
        )
        self.assertEqual(self.manifest["execution"]["maximum_reruns"], 0)

    def test_frozen_authority_hashes_match_baseline_revision(self) -> None:
        revision = self.manifest["baseline"]["revision"]
        for authority in self.manifest["authority_inputs"].values():
            actual = hashlib.sha256(
                read_at_revision(revision, authority["path"])
            ).hexdigest()
            self.assertEqual(actual, authority["sha256"], authority["path"])

    def test_current_source_has_no_rejected_implementation_owner(self) -> None:
        sources = rust_sources(
            "crates/app",
            "crates/context",
            "crates/deepseek",
            "crates/orchestrator",
            "crates/protocol",
            "crates/runtime",
            "crates/state",
            "crates/tools",
        )
        for retired in (
            "RepoGraph",
            "plan_fim",
            "/beta/completions",
            "ApiSurface::Fim",
            "multi_writer",
            "max_writers",
        ):
            self.assertNotIn(retired, sources)

    def test_existing_single_writer_is_explicit_and_bounded(self) -> None:
        runtime = read("crates/runtime/src/agent.rs")
        tests = read("crates/runtime/tests/writer_orchestration.rs")
        self.assertIn("当前运行未显式启用隔离 Writer", runtime)
        self.assertIn("每个 root run 只允许冻结一个隔离 Writer 任务", runtime)
        self.assertIn(
            "one_root_rejects_a_second_writer_in_the_same_batch_or_a_later_turn",
            tests,
        )

    def test_current_chat_surface_has_lossless_strict_fallback_only(self) -> None:
        protocol = read("crates/protocol/src/agent_runtime.rs")
        deepseek = read("crates/deepseek/src/lib.rs")
        transport = read("crates/deepseek/src/transport.rs")
        surface = protocol.split("pub enum ApiSurface", 1)[1].split("}", 1)[0]
        self.assertIn("StandardChat", surface)
        self.assertIn("StrictChat", surface)
        self.assertNotIn("Fim", surface)
        self.assertIn("ApiSurface::StandardChat", deepseek)
        self.assertIn("ApiSurface::StrictChat", deepseek)
        self.assertIn('format!("{root}/chat/completions")', transport)

    def test_cross_file_capability_uses_canonical_existing_tools(self) -> None:
        catalog = read("crates/tools/src/production.rs")
        for tool in (
            "file_search",
            "grep_files",
            "list_dir",
            "read_file",
            "git_diff",
            "git_status",
        ):
            self.assertIn(f'"{tool}"', catalog)
        context = rust_sources("crates/context")
        self.assertNotIn("RepoGraph", context)

    def test_accepted_adr_and_product_plan_define_capabilities_not_literals(self) -> None:
        adr = read("docs/decisions/0005-v1-evidence-gated-capability-scope.md")
        self.assertIn("- 状态：已接受", adr)
        self.assertIn("并发多 Writer 不是 V1 要求", adr)
        self.assertIn("FIM 不是 V1 要求", adr)
        self.assertIn("RepoGraph 的索引实现不是 V1 要求", adr)

        product = read("docs/product/PRODUCT_PLAN.md")
        v1 = product.split("## 14. V1 完成定义", 1)[1].split("## 15.", 1)[0]
        self.assertEqual(len(re.findall(r"^- ", v1, flags=re.MULTILINE)), 16)
        self.assertIn("多 Writer 不作为", v1)
        self.assertIn("FIM 只在新证据准入后进入", v1)
        self.assertIn("RepoGraph 实现不作为 V1 门槛", v1)

    def test_successor_matrix_remains_not_releasable(self) -> None:
        roadmap = read("docs/product/ROADMAP.md")
        evaluation = read("docs/product/EVALUATION.md")
        for authority in (roadmap, evaluation):
            self.assertIn("13 pass / 3 blocked", authority)
            self.assertIn("V13", authority)
            self.assertIn("V15", authority)
            self.assertIn("V16", authority)
        self.assertIn("V1 仍不可发布", roadmap)


if __name__ == "__main__":
    unittest.main()
