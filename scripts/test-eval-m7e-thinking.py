#!/usr/bin/env python3
"""Regression tests for the current Run API v10 / RuntimeEvent v16 M7-E Harness."""

from __future__ import annotations

import importlib.util
import json
import os
from pathlib import Path
import sqlite3
import stat
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]
HARNESS_PATH = ROOT / "scripts/eval-m7e-thinking.py"
SPEC = importlib.util.spec_from_file_location("codewhale_m7e_thinking", HARNESS_PATH)
assert SPEC is not None and SPEC.loader is not None
HARNESS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(HARNESS)


def stored(sequence: int, kind: str, **payload: object) -> dict[str, object]:
    return {
        "schema_version": 16,
        "run_id": "run-test",
        "sequence": sequence,
        "event_id": f"event-{sequence}",
        "occurred_at_unix_ms": sequence,
        "event": {"kind": kind, **payload},
    }


def usage(**overrides: int) -> dict[str, int]:
    value = {
        "input_tokens": 100,
        "output_tokens": 20,
        "cache_hit_tokens": 80,
        "cache_miss_tokens": 20,
        "cache_write_tokens": 0,
        "reasoning_tokens": 0,
        "reasoning_replay_tokens": 0,
    }
    value.update(overrides)
    return value


def run_view(*, effort: str = "off") -> dict[str, object]:
    observed_usage = usage(
        reasoning_tokens=0 if effort == "off" else 8,
        reasoning_replay_tokens=0 if effort == "off" else 3,
    )
    surface = {
        "surface": "standard_chat",
        "model": "deepseek-v4-flash",
        "response_count": 2,
        "usage_response_count": 2,
        "usage": observed_usage,
        "cost_nanousd": 99,
        "cost_nanocny": 0,
    }
    accounting = {
        "hard_request_limit": 10,
        "root": {"started": 2, "completed": 2, "in_flight": 0, "retries": 0},
        "child": {"started": 0, "completed": 0, "in_flight": 0, "retries": 0},
        "transport_retries": 0,
        "runtime_retries": 0,
        "sealed_denied": 0,
        "exhausted_denied": 0,
        "budget_exhausted": False,
        "sealed": True,
        "complete": True,
        "usage_complete": True,
        "usage_missing": False,
        "usage_incomplete": False,
        "billing_unknown": False,
        "unpriced": False,
        "usage_responses": 2,
        "usage_missing_responses": 0,
        "incomplete_responses": 0,
        "billing_unknown_attempts": 0,
        "unpriced_usage_responses": 0,
        "records_after_seal": 0,
        "usage": observed_usage,
        "surface_usage": [surface],
        "cost_nanousd": 99,
        "cost_nanocny": 0,
    }
    return {
        "terminal": {
            "state": "completed",
            "message": "完成",
            "decision": {"candidate_id": "candidate"},
        },
        "usage": observed_usage,
        "accounting": accounting,
    }


class M7EThinkingHarnessTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.manifest, cls.tasks = HARNESS.load_manifest(frozen=False)

    def test_schedule_is_balanced_and_has_three_arms_per_cell(self) -> None:
        schedule = HARNESS.formal_schedule(self.manifest)
        self.assertEqual(len(schedule), 30)
        cells: dict[tuple[str, str], int] = {}
        firsts = {"reasoning_high": 0, "reasoning_off": 0}
        for arm in schedule:
            key = (arm["task_id"], arm["variant"])
            cells[key] = cells.get(key, 0) + 1
            if arm["arm_position"] == 1:
                firsts[arm["variant"]] += 1
        self.assertEqual(set(cells.values()), {3})
        self.assertLessEqual(abs(firsts["reasoning_high"] - firsts["reasoning_off"]), 1)

    def test_frozen_hashes_match_current_harness_and_schedule(self) -> None:
        manifest, tasks = HARNESS.load_manifest(frozen=True)
        self.assertEqual(manifest["schema"], HARNESS.SCHEMA)
        self.assertEqual(set(tasks["tasks"]), set(self.tasks["tasks"]))

    def test_start_treatment_changes_only_reasoning_effort(self) -> None:
        workspace = ROOT
        high = HARNESS.start_envelope(
            self.manifest,
            self.tasks,
            "t1",
            "reasoning_high",
            workspace,
            "request",
        )
        off = HARNESS.start_envelope(
            self.manifest,
            self.tasks,
            "t1",
            "reasoning_off",
            workspace,
            "request",
        )
        self.assertEqual(high["command"].pop("reasoning_effort"), "high")
        self.assertEqual(off["command"].pop("reasoning_effort"), "off")
        self.assertEqual(high, off)
        encoded = HARNESS.canonical_bytes(high)
        self.assertNotIn(b"DEEPSEEK_API_KEY", encoded)
        self.assertNotIn(b"key.txt", encoded)

    def test_request_projection_requires_exact_effort_and_model(self) -> None:
        events = [
            stored(
                1,
                "model_request_prepared",
                attempt_id="attempt",
                request={
                    "actor": {"kind": "root", "depth": 0},
                    "model": "deepseek-v4-flash",
                    "system_prompt": {"blocks": [{"text": "prompt"}]},
                    "messages": [{"role": "user", "content": "task"}],
                    "tools": [{"name": "read_file"}],
                    "reasoning_effort": "off",
                    "max_output_tokens": 8192,
                    "streaming": True,
                },
            )
        ]
        projection = HARNESS.request_projection(events, [], "off")
        self.assertTrue(projection["valid"])
        self.assertFalse(HARNESS.request_projection(events, [], "high")["valid"])

    def test_accounting_closes_and_unknown_billing_aborts(self) -> None:
        off = HARNESS.accounting_projection(run_view(effort="off"), "off")
        self.assertTrue(off["valid"])
        self.assertTrue(off["off_reasoning_zero"])
        high = HARNESS.accounting_projection(run_view(effort="high"), "high")
        self.assertTrue(high["valid"])
        arm = {"accounting": off, "measurement_valid": True}
        self.assertIsNone(HARNESS.accounting_abort_code(self.manifest, arm))
        off["billing_unknown"] = True
        self.assertEqual(
            HARNESS.accounting_abort_code(self.manifest, arm),
            "aborted_unknown_billing",
        )
        off["billing_unknown"] = False
        arm["tool"] = {
            "outcomes": [
                {
                    "side_effect": "indeterminate",
                    "failure_code": "side_effect_ambiguous",
                }
            ]
        }
        self.assertEqual(
            HARNESS.accounting_abort_code(self.manifest, arm),
            "aborted_side_effect_ambiguous",
        )
        arm["tool"]["outcomes"][0].update(
            {
                "invocation": "accepted",
                "transport": "succeeded",
                "operation": "succeeded",
                "retry": "not_needed",
                "failure_code": None,
            }
        )
        self.assertIsNone(HARNESS.accounting_abort_code(self.manifest, arm))
        arm["tool"]["outcomes"][0].update(
            {
                "operation": "failed",
                "side_effect": "indeterminate",
                "retry": "unsafe",
                "failure_code": "verifier_failed",
            }
        )
        self.assertEqual(
            HARNESS.accounting_abort_code(self.manifest, arm),
            "aborted_side_effect_ambiguous",
        )
        arm.update(
            {
                "task_id": "t3",
                "verification": {"valid": True, "temporal_valid": False},
            }
        )
        self.assertEqual(
            HARNESS.accounting_abort_code(self.manifest, arm),
            "aborted_side_effect_ambiguous",
        )
        arm["verification"]["temporal_valid"] = True
        self.assertIsNone(HARNESS.accounting_abort_code(self.manifest, arm))
        arm["tool"]["outcomes"][0].update(
            {
                "transport": "indeterminate",
                "operation": "indeterminate",
                "failure_code": "side_effect_ambiguous",
            }
        )
        self.assertEqual(
            HARNESS.accounting_abort_code(self.manifest, arm),
            "aborted_side_effect_ambiguous",
        )

    def test_surface_totals_and_off_reasoning_are_fail_closed(self) -> None:
        wrong_surface = run_view(effort="off")
        wrong_surface["accounting"]["surface_usage"][0]["surface"] = "strict_chat"
        self.assertFalse(
            HARNESS.accounting_projection(wrong_surface, "off")["valid"]
        )
        leaked = run_view(effort="off")
        leaked["usage"]["reasoning_tokens"] = 1
        leaked["accounting"]["usage"]["reasoning_tokens"] = 1
        leaked["accounting"]["surface_usage"][0]["usage"]["reasoning_tokens"] = 1
        projection = HARNESS.accounting_projection(leaked, "off")
        self.assertFalse(projection["valid"])
        self.assertFalse(projection["off_reasoning_zero"])

    def test_t3_verification_requires_failure_before_successful_edit(self) -> None:
        failure = {
            "invocation": "accepted",
            "transport": "succeeded",
            "operation": "failed",
        }
        success = {
            "invocation": "accepted",
            "transport": "succeeded",
            "operation": "succeeded",
        }
        workspace_state = {
            "generation": 2,
            "revision": {"status": "known", "sha256": "sha256:workspace"},
        }
        receipt = {
            "id": "receipt",
            "generation_id": "generation",
            "acceptance_id": "m7a-t3",
            "workspace_state": workspace_state,
        }
        events = [
            stored(1, "run_created", request={"task_contract": {"id": "t3"}}),
            stored(
                2,
                "tool_outcome_committed",
                name="run_verifiers",
                outcome=failure,
            ),
            stored(
                3,
                "tool_outcome_committed",
                name="apply_patch",
                outcome=success,
            ),
            stored(4, "host_verification_prepared"),
            stored(5, "host_verification_started"),
            stored(
                6,
                "host_verification_committed",
                outcome=success,
                receipt=receipt,
                workspace_state_after=workspace_state,
            ),
            stored(
                7,
                "terminal",
                outcome={
                    "terminal": {
                        "state": "completed",
                        "decision": {
                            "generation_id": "generation",
                            "workspace_state": workspace_state,
                            "satisfied": [
                                {
                                    "kind": "evidence",
                                    "acceptance_id": "m7a-t3",
                                    "receipt_id": "receipt",
                                }
                            ],
                        },
                    }
                },
            ),
        ]
        projection = HARNESS.verification_projection("t3", events)
        self.assertTrue(projection["valid"])
        events[1], events[2] = events[2], events[1]
        events[1]["sequence"], events[2]["sequence"] = 2, 3
        self.assertFalse(HARNESS.verification_projection("t3", events)["valid"])

    def test_readonly_child_expectation_rejects_write_tool(self) -> None:
        task = self.tasks["tasks"]["t5"]
        allowed = self.tasks["tool_policy"]["readonly_child_tools"]
        child = {
            "terminal": {"state": "completed"},
            "workspace_access": "read_only",
            "allowed_tools": allowed,
            "tool_names": ["read_file"],
        }
        self.assertTrue(
            HARNESS.child_expectation_valid("t5", task, [child], allowed)
        )
        child["tool_names"] = ["edit_file"]
        self.assertFalse(
            HARNESS.child_expectation_valid("t5", task, [child], allowed)
        )

    def test_private_output_is_exclusive_and_0600(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            path = Path(raw) / "result.json"
            HARNESS.write_private_json(path, {"secret": False}, replace=False)
            self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o600)
            with self.assertRaises(HARNESS.EvaluationError) as context:
                HARNESS.write_private_json(path, {"again": True}, replace=False)
            self.assertEqual(context.exception.code, "output_exists")
            HARNESS.write_private_json(path, {"final": True}, replace=True)
            self.assertEqual(json.loads(path.read_text()), {"final": True})
            self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o600)

    def test_state_schema_is_exactly_v21(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            database = root / "state.db"
            connection = sqlite3.connect(database)
            connection.execute("PRAGMA user_version = 21")
            connection.close()
            self.assertEqual(
                HARNESS.state_schema(root),
                {"valid": True, "version": 21},
            )

    def test_fixture_identity_materializes_exact_git_base(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            destination = Path(raw) / "workspace"
            base = HARNESS.materialize_fixture(self.tasks, "t1", destination)
            self.assertEqual(
                base,
                self.tasks["tasks"]["t1"]["fixture_base_commit"],
            )


if __name__ == "__main__":
    unittest.main()
