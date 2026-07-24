#!/usr/bin/env python3
"""Offline authority and evidence contract for the M8-L successor."""

from __future__ import annotations

import hashlib
import json
import pathlib
import re
import stat
import subprocess
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
MANIFEST_PATH = (
    ROOT / "eval/manifests/m8-l-release-benchmark-successor-v1.json"
)


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def read_at_revision(revision: str, path: str) -> str:
    return subprocess.run(
        ["git", "show", f"{revision}:{path}"],
        cwd=ROOT,
        check=True,
        text=True,
        capture_output=True,
    ).stdout


def sha256(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


class M8LReleaseBenchmarkContract(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.manifest = json.loads(MANIFEST_PATH.read_text(encoding="utf-8"))

    def test_frozen_identity_and_no_live_admission(self) -> None:
        self.assertEqual(
            self.manifest["schema"],
            "codewhale.eval.m8-l-release-benchmark-successor.v1",
        )
        self.assertEqual(
            self.manifest["baseline"]["revision"],
            "4fef6a34e999a8d592629308cb1b5fd7017bbf18",
        )
        self.assertEqual(
            self.manifest["imported_audit"]["revision"],
            "352e86a611fdf3cd8bd27c36d24d482c06a71117",
        )
        self.assertEqual(
            self.manifest["imported_audit"]["paid_ab_admission"]["status"],
            "inadmissible_incomplete_baseline_accounting",
        )
        execution = self.manifest["execution"]
        self.assertEqual(execution["maximum_reruns"], 0)
        self.assertFalse(execution["credential_read"])
        self.assertFalse(execution["official_api_requests_allowed"])
        self.assertFalse(execution["external_network_allowed"])

    def test_imported_source_has_real_retry_but_no_physical_ledger(self) -> None:
        revision = self.manifest["imported_audit"]["revision"]
        main = read_at_revision(revision, "crates/tui/src/main.rs")
        turn_loop = read_at_revision(
            revision, "crates/tui/src/core/engine/turn_loop.rs"
        )
        events = read_at_revision(revision, "crates/tui/src/core/events.rs")
        self.assertIn("retry_count: Option<u32>", main)
        self.assertGreaterEqual(main.count("retry_count: None"), 3)
        self.assertNotIn("api_request_count", main)
        self.assertGreaterEqual(
            turn_loop.count(
                "client.create_message_stream(stream_request.clone())"
            ),
            2,
        )
        self.assertIn("turn.add_usage(&usage)", turn_loop)
        self.assertIn("TurnComplete {", events)
        self.assertNotIn("api_request", events)

    def test_current_terminal_and_store_have_complete_accounting_shape(self) -> None:
        terminal = read("crates/tui/src/exec_runtime.rs")
        protocol = read("crates/protocol/src/agent_runtime.rs")
        for fact in (
            "api_request_count: Some(u32_saturating(accounting.total_started()))",
            "usage_complete",
            "cost_complete",
            "api_request_root_started",
            "transport_retry_count",
        ):
            self.assertIn(fact, terminal)
        for fact in (
            "pub struct ModelAccounting",
            "pub root: ActorRequestAccounting",
            "pub child: ActorRequestAccounting",
            "pub transport_retries: u64",
        ):
            self.assertIn(fact, protocol)

    def test_qualified_real_coding_evidence_is_exact_and_private(self) -> None:
        evidence = self.manifest["qualified_real_coding_evidence"]
        summary = ROOT / evidence["summary_path"]
        raw = ROOT / evidence["raw_path"]
        self.assertEqual(sha256(summary), evidence["summary_sha256"])
        self.assertTrue(raw.is_file())
        self.assertEqual(stat.S_IMODE(raw.stat().st_mode), 0o600)
        self.assertEqual(sha256(raw), evidence["raw_sha256"])
        result = json.loads(raw.read_text(encoding="utf-8"))
        self.assertEqual(len(result["records"]), 12)
        self.assertTrue(result["aggregate"]["product_metric_eligible"])
        cells = {
            (cell["scenario"], cell["variant"]): cell
            for cell in result["aggregate"]["cells"]
        }
        self.assertEqual(
            cells[("coding_fix", "candidate")]["verified_success"], 3
        )
        self.assertEqual(
            cells[("coding_fix", "candidate")]["false_success"], 0
        )
        self.assertEqual(
            cells[("forced_false_claim", "candidate")][
                "correct_rejection"
            ],
            3,
        )

    def test_current_benchmark_has_one_gate_per_case(self) -> None:
        benchmark = self.manifest["current_release_benchmark"]
        self.assertEqual(
            {case["gate_id"] for case in benchmark["cases"]},
            {gate["id"] for gate in benchmark["gates"]},
        )
        self.assertEqual(len(benchmark["cases"]), 5)
        production = read("crates/app/src/production.rs")
        conformance = read("crates/runtime/tests/conformance.rs")
        cli = read("crates/cli/tests/canonical_runs_command.rs")
        exec_acceptance = read("crates/tui/tests/exec_terminal_acceptance.rs")
        sources = "\n".join(
            (production, conformance, cli, exec_acceptance)
        )
        for gate in benchmark["gates"]:
            self.assertIn(gate["filter"], sources)

    def test_workflow_actions_do_not_increase(self) -> None:
        comparison = self.manifest["workflow_step_comparison"]
        self.assertEqual(
            [workflow["id"] for workflow in comparison["workflows"]],
            ["W01", "W02", "W03", "W04", "W05"],
        )
        self.assertEqual(
            sum(item["imported_actions"] for item in comparison["workflows"]),
            5,
        )
        self.assertEqual(
            sum(item["current_actions"] for item in comparison["workflows"]),
            5,
        )
        self.assertTrue(
            all(
                item["current_actions"] <= item["imported_actions"]
                for item in comparison["workflows"]
            )
        )

    def test_accepted_adr_and_product_plan_define_successor(self) -> None:
        adr = read(
            "docs/decisions/0006-v1-release-benchmark-successor.md"
        )
        self.assertIn("- 状态：已接受", adr)
        self.assertIn("不伪造不可计量的 imported A/B", adr)
        self.assertIn("5 -> 5", adr)
        self.assertIn("15 pass / 1 blocked", adr)

        product = read("docs/product/PRODUCT_PLAN.md")
        v1 = product.split("## 14. V1 完成定义", 1)[1].split(
            "## 15.", 1
        )[0]
        self.assertEqual(
            len(re.findall(r"^- ", v1, flags=re.MULTILINE)), 16
        )
        self.assertIn("release benchmark", v1)
        self.assertIn("imported superiority", v1)
        self.assertIn("共同用户 workflow", v1)

    def test_authority_matrix_is_consistent_and_not_releasable(self) -> None:
        for path in (
            "docs/product/ROADMAP.md",
            "docs/product/EVALUATION.md",
            "docs/architecture/CURRENT_CODEWHALE.md",
        ):
            authority = read(path)
            self.assertIn("15 pass / 1 blocked", authority, path)
            self.assertIn("V15", authority, path)
            self.assertIn("V1 仍不可发布", authority, path)
        summary = read(
            "eval/summaries/m8-l-release-benchmark-successor-2026-07-24.md"
        )
        self.assertIn(
            "inadmissible_incomplete_baseline_accounting", summary
        )
        self.assertIn("credential read / official API requests：false / 0", summary)
        self.assertIn("5 -> 5", summary)


if __name__ == "__main__":
    unittest.main()
