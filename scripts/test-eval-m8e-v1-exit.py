#!/usr/bin/env python3
"""Offline contract tests for the frozen M8-E V1 exit gap matrix."""

from __future__ import annotations

import hashlib
import json
import pathlib
import re
import subprocess
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "eval/manifests/m8-e-v1-exit-audit-v1.json"


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def sha256_at_revision(revision: str, path: str) -> str:
    content = subprocess.run(
        ["git", "show", f"{revision}:{path}"],
        cwd=ROOT,
        check=True,
        capture_output=True,
    ).stdout
    return hashlib.sha256(content).hexdigest()


def read_at_revision(revision: str, path: str) -> str:
    return subprocess.run(
        ["git", "show", f"{revision}:{path}"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    ).stdout


class M8EV1ExitContract(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
        cls.matrix = cls.manifest["v1_gap_matrix"]

    def test_manifest_identity_and_matrix_are_closed(self) -> None:
        self.assertEqual(
            self.manifest["schema"], "codewhale.eval.m8-e-v1-exit-audit.v1"
        )
        self.assertEqual(
            [entry["id"] for entry in self.matrix],
            [f"V{index:02d}" for index in range(1, 17)],
        )
        statuses = [entry["status"] for entry in self.matrix]
        self.assertEqual(statuses.count("pass"), 8)
        self.assertEqual(statuses.count("blocked"), 8)
        self.assertEqual(
            self.manifest["release_decision"]["status"], "not_releasable"
        )
        self.assertEqual(self.manifest["execution"]["maximum_reruns"], 0)
        self.assertFalse(self.manifest["execution"]["credential_read"])
        self.assertFalse(
            self.manifest["execution"]["official_api_requests_allowed"]
        )

    def test_authority_hashes_bind_the_frozen_audit_input(self) -> None:
        expected = self.manifest["authorities"]
        revision = self.manifest["production_revision"]
        self.assertEqual(
            expected["product_plan_sha256"],
            sha256_at_revision(revision, "docs/product/PRODUCT_PLAN.md"),
        )
        self.assertEqual(
            expected["roadmap_sha256"],
            sha256_at_revision(revision, "docs/product/ROADMAP.md"),
        )
        self.assertEqual(
            expected["evaluation_sha256"],
            sha256_at_revision(revision, "docs/product/EVALUATION.md"),
        )
        self.assertEqual(
            expected["current_architecture_sha256"],
            sha256_at_revision(
                revision,
                "docs/architecture/CURRENT_CODEWHALE.md",
            ),
        )

    def test_product_plan_has_exactly_sixteen_v1_requirements(self) -> None:
        product_plan = read("docs/product/PRODUCT_PLAN.md")
        section = product_plan.split("## 14. V1 完成定义", 1)[1].split(
            "## 15.", 1
        )[0]
        requirements = re.findall(r"^- ", section, flags=re.MULTILINE)
        self.assertEqual(len(requirements), 16)

    def test_single_runtime_event_and_production_store_owners(self) -> None:
        runtime_sources = "\n".join(
            path.read_text(encoding="utf-8")
            for path in (ROOT / "crates/runtime/src").rglob("*.rs")
        )
        protocol_sources = "\n".join(
            path.read_text(encoding="utf-8")
            for path in (ROOT / "crates/protocol/src").rglob("*.rs")
        )
        state_sources = "\n".join(
            path.read_text(encoding="utf-8")
            for path in (ROOT / "crates/state/src").rglob("*.rs")
        )
        self.assertEqual(runtime_sources.count("pub struct AgentRuntime"), 1)
        self.assertEqual(protocol_sources.count("pub enum RuntimeEventKind"), 1)
        self.assertEqual(state_sources.count("impl RunStore for StateStore"), 1)
        self.assertNotIn("impl RunStore for StateStore", runtime_sources)

    def test_current_wire_and_user_selected_target_are_not_conflated(self) -> None:
        official = self.manifest["official_deepseek_recheck"]
        self.assertEqual(
            official["user_selected_release_target"],
            "https://api.deepseek.com/anthropic",
        )
        transport = read("crates/deepseek/src/transport.rs")
        planner = read("crates/deepseek/src/lib.rs")
        self.assertIn('format!("{root}/chat/completions")', transport)
        self.assertIn('format!("{unversioned}/beta/chat/completions")', planner)
        self.assertNotIn("/anthropic", transport)
        self.assertNotIn("/anthropic", planner)

    def test_fim_is_planner_only_and_repograph_is_absent(self) -> None:
        non_deepseek_sources = []
        for crate in (ROOT / "crates").iterdir():
            if not crate.is_dir() or crate.name == "deepseek":
                continue
            non_deepseek_sources.extend(crate.rglob("*.rs"))
        joined = "\n".join(
            path.read_text(encoding="utf-8", errors="replace")
            for path in non_deepseek_sources
        )
        self.assertNotIn("plan_fim(", joined)
        self.assertNotRegex(joined, r"\bRepoGraph\b")

    def test_duplicate_task_product_concepts_are_observable(self) -> None:
        revision = self.manifest["production_revision"]
        cli = read_at_revision(revision, "crates/cli/src/lib.rs")
        self.assertIn("Fleet(TuiPassthroughArgs)", cli)
        self.assertIn("Lane(LaneArgs)", cli)
        self.assertIn(
            "pub struct ProductionAgentOrchestrator",
            read_at_revision(revision, "crates/orchestrator/src/runtime.rs"),
        )

    def test_known_release_blockers_remain_fail_closed(self) -> None:
        status_by_id = {entry["id"]: entry["status"] for entry in self.matrix}
        for blocker in ("V06", "V08", "V09", "V10", "V12", "V13", "V15", "V16"):
            self.assertEqual(status_by_id[blocker], "blocked")
        self.assertIn(
            "hold_prompt_candidate",
            read("eval/summaries/m8-d-native-zh-prompt-ab-2026-07-24.md"),
        )


if __name__ == "__main__":
    unittest.main()
