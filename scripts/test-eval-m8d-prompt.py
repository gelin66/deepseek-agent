#!/usr/bin/env python3
"""Offline contract tests for the M8-D prompt evaluator."""

from __future__ import annotations

from copy import deepcopy
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[1]
HARNESS_PATH = ROOT / "scripts/eval-m8d-prompt.py"


def load_harness():
    spec = importlib.util.spec_from_file_location(
        "codewhale_eval_m8d_prompt", HARNESS_PATH
    )
    if spec is None or spec.loader is None:
        raise RuntimeError("M8-D harness is unavailable")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


M8D = load_harness()


def prompt_fingerprint(stable: str, suffix: str = "same") -> dict:
    return {
        "actor_label": "root",
        "actor": {"kind": "root", "depth": 0},
        "model": "deepseek-v4-flash",
        "reasoning_effort": "high",
        "semantic_messages_sha256": "sha256:messages",
        "task_generation_count": 1,
        "tools_sha256": "sha256:tools",
        "max_output_tokens": 8192,
        "streaming": True,
        "prompt": {
            "prefix_valid": True,
            "stable_block_sha256": stable,
            "stable_suffix_sha256": suffix,
            "remaining_blocks_sha256": "sha256:remaining",
            "block_count": 4,
            "cache_controls": ["stable", "volatile", "volatile", "volatile"],
        },
    }


def paired_arm(variant: str) -> dict:
    fingerprint = prompt_fingerprint(
        "sha256:baseline" if variant == "baseline" else "sha256:candidate"
    )
    return {
        "task_id": "t1",
        "run_index": 1,
        "variant": variant,
        "revision": "a" * 40,
        "binary_pair_sha256": "sha256:pair",
        "fixture_tree_sha256": "sha256:fixture",
        "task_definition_sha256": "sha256:task",
        "command_sha256": "sha256:command",
        "workspace_slot_sha256": "sha256:workspace",
        "request_identity": {"first_root": fingerprint},
    }


class PromptHarnessTests(unittest.TestCase):
    def setUp(self) -> None:
        self.manifest, self.tasks = M8D.load_manifest(frozen=False)

    def test_v2_never_reuses_the_aborted_v1_suite_identity(self) -> None:
        self.assertEqual(self.manifest["schema"], "codewhale.eval.m8-d-prompt-ab.v2")
        self.assertEqual(M8D.RESULT_SCHEMA, "codewhale.eval.m8-d-prompt-result.v2")
        self.assertEqual(
            self.manifest["prior_attempt"]["status"],
            "aborted_measurement_invalid",
        )
        self.assertEqual(self.manifest["experiment"]["maximum_reruns"], 0)

    def test_candidate_is_exactly_one_constitution_replacement(self) -> None:
        baseline = (
            ROOT / self.manifest["prompt_treatment"]["baseline"]["source"]
        ).read_text(encoding="utf-8")
        candidate = M8D.CANDIDATE_PROMPT_PATH.read_text(encoding="utf-8")
        self.assertTrue(M8D.exact_candidate_delta(baseline, candidate))
        self.assertEqual(
            M8D.file_hash(M8D.CANDIDATE_PROMPT_PATH),
            self.manifest["prompt_treatment"]["candidate"]["sha256"],
        )

    def test_schedule_is_balanced_and_has_no_reruns(self) -> None:
        schedule = M8D.formal_schedule(self.manifest)
        self.assertEqual(len(schedule), 30)
        cells = {}
        pairs = {}
        for arm in schedule:
            cells[(arm["task_id"], arm["variant"])] = (
                cells.get((arm["task_id"], arm["variant"]), 0) + 1
            )
            pairs.setdefault(arm["pair_index"], []).append(arm)
        self.assertEqual(set(cells.values()), {3})
        self.assertEqual(len(pairs), 15)
        self.assertTrue(
            all(
                len(arms) == 2
                and {arm["variant"] for arm in arms} == set(M8D.VARIANTS)
                for arms in pairs.values()
            )
        )
        self.assertEqual(self.manifest["experiment"]["maximum_reruns"], 0)

    def test_task_lanes_and_fixtures_are_frozen(self) -> None:
        self.assertEqual(
            {task_id: task["lane"] for task_id, task in self.tasks["tasks"].items()},
            {
                "t1": "single",
                "t3": "single",
                "t5": "read_only_multi",
                "w1": "explicit_writer",
                "w3": "explicit_writer",
            },
        )
        for task_id, task in self.tasks["tasks"].items():
            self.assertEqual(
                M8D.M7E.fixture_hash(self.tasks, task_id),
                task["fixture_tree_sha256"],
            )

    def test_writer_command_changes_authority_not_prompt_variant(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            workspace = Path(raw)
            baseline = M8D.start_envelope(
                self.manifest, self.tasks, "w1", workspace, "request-a"
            )
            candidate = M8D.start_envelope(
                self.manifest, self.tasks, "w1", workspace, "request-b"
            )
        self.assertEqual(baseline["command"], candidate["command"])
        self.assertEqual(
            baseline["command"]["controls"]["write_execution_mode"],
            "isolated_writer",
        )
        objective = baseline["command"]["task"]["objective"]
        expected = json.dumps(
            M8D.writer_arguments(self.manifest, self.tasks["tasks"]["w1"]),
            ensure_ascii=False,
            sort_keys=True,
            separators=(",", ":"),
        )
        self.assertIn(expected, objective)

    def test_pair_identity_allows_only_constitution_prefix_delta(self) -> None:
        baseline = paired_arm("baseline")
        candidate = paired_arm("candidate")
        self.assertTrue(M8D.paired_identity_matches(baseline, candidate))
        changed = deepcopy(candidate)
        changed["request_identity"]["first_root"]["tools_sha256"] = "sha256:changed"
        self.assertFalse(M8D.paired_identity_matches(baseline, changed))
        changed = deepcopy(candidate)
        changed["request_identity"]["first_root"]["prompt"][
            "remaining_blocks_sha256"
        ] = "sha256:changed"
        self.assertFalse(M8D.paired_identity_matches(baseline, changed))
        changed = deepcopy(candidate)
        changed["request_identity"]["first_root"]["prompt"][
            "stable_block_sha256"
        ] = "sha256:baseline"
        self.assertFalse(M8D.paired_identity_matches(baseline, changed))

    def test_process_activation_identity_allows_only_constitution_delta(self) -> None:
        baseline = {
            "non_prompt": {"model": "deepseek-v4-flash", "tools": "same"},
            "prompt": prompt_fingerprint("sha256:baseline")["prompt"],
        }
        candidate = deepcopy(baseline)
        candidate["prompt"]["stable_block_sha256"] = "sha256:candidate"
        self.assertTrue(M8D.activation_identity_matches(baseline, candidate))
        changed = deepcopy(candidate)
        changed["non_prompt"]["tools"] = "changed"
        self.assertFalse(M8D.activation_identity_matches(baseline, changed))
        changed = deepcopy(candidate)
        changed["prompt"]["stable_suffix_sha256"] = "changed"
        self.assertFalse(M8D.activation_identity_matches(baseline, changed))

    def test_prompt_signature_binds_prefix_and_preserves_suffix(self) -> None:
        baseline = "BASE"
        candidate = "CANDIDATE"
        request = {
            "system_prompt": {
                "blocks": [
                    {"text": "CANDIDATE\n\nOUTPUT", "cache_control": "stable"},
                    {"text": "ROLE", "cache_control": "volatile"},
                ]
            }
        }
        signature = M8D.prompt_signature(
            request, "candidate", baseline, candidate
        )
        self.assertTrue(signature["prefix_valid"])
        self.assertEqual(signature["block_count"], 2)
        request["system_prompt"]["blocks"][0]["text"] = "WRONG"
        signature = M8D.prompt_signature(
            request, "candidate", baseline, candidate
        )
        self.assertFalse(signature["prefix_valid"])

    def test_writer_lifecycle_requires_full_ordered_cleanup(self) -> None:
        kinds = list(M8D.LIFECYCLE_KINDS)
        events = []
        for sequence, kind in enumerate(kinds, start=1):
            event = {"kind": kind}
            if kind == "agent_task_prepared":
                event["task"] = {
                    "workspace_access": "isolated_write",
                    "allowed_paths": ["format_bytes.py"],
                }
            if kind == "agent_cleanup_committed":
                event["result"] = {"status": "removed"}
            events.append(
                {
                    "schema_version": 16,
                    "sequence": sequence,
                    "event": event,
                }
            )
        events.insert(
            1,
            {
                "schema_version": 16,
                "sequence": 100,
                "event": {
                    "kind": "tool_prepared",
                    "invocation": {"name": "agent"},
                },
            },
        )
        task = self.tasks["tasks"]["w1"]

        def git_output(*arguments, cwd):
            if arguments[:2] == ("worktree", "list"):
                return f"worktree {cwd}\n"
            return ""

        with tempfile.TemporaryDirectory() as raw, mock.patch.object(
            M8D.M7E, "git_output", side_effect=git_output
        ):
            result = M8D.writer_lifecycle(
                self.manifest,
                task,
                events,
                [
                    {
                        "terminal": {"state": "completed"},
                        "workspace_access": "isolated_write",
                    }
                ],
                Path(raw),
            )
        self.assertTrue(result["valid"])
        broken = [
            event
            for event in events
            if event["event"]["kind"] != "agent_integration_committed"
        ]
        with tempfile.TemporaryDirectory() as raw, mock.patch.object(
            M8D.M7E, "git_output", side_effect=git_output
        ):
            result = M8D.writer_lifecycle(
                self.manifest,
                task,
                broken,
                [
                    {
                        "terminal": {"state": "completed"},
                        "workspace_access": "isolated_write",
                    }
                ],
                Path(raw),
            )
        self.assertFalse(result["valid"])

    def test_unknown_billing_stops_before_next_arm(self) -> None:
        arm = {
            "accounting": {
                "billing_unknown": True,
                "unpriced": False,
                "sealed": True,
                "complete": True,
                "usage_complete": True,
                "cost_nanousd": 1,
            },
            "measurement_valid": True,
            "writer": {"required": False, "valid": True},
        }
        self.assertEqual(
            M8D.accounting_abort_code(self.manifest, arm),
            "aborted_unknown_billing",
        )

    def test_freeze_report_reads_no_key_and_has_all_identities(self) -> None:
        report = M8D.freeze_report()
        self.assertEqual(report["schedule_arms"], 30)
        self.assertEqual(set(report["fixtures"]), set(self.tasks["tasks"]))
        self.assertEqual(
            report["candidate_prompt_sha256"],
            self.manifest["prompt_treatment"]["candidate"]["sha256"],
        )


if __name__ == "__main__":
    unittest.main(verbosity=2)
