#!/usr/bin/env python3
"""Regression tests for the RuntimeEvent v16 M7-D observation projector."""

from __future__ import annotations

import copy
import importlib.util
from pathlib import Path
import tempfile
import unittest


SCRIPT = Path(__file__).with_name("eval-m7d-edit-observation.py")
SPEC = importlib.util.spec_from_file_location("codewhale_m7d_observation", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
M7D = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(M7D)


def stored(sequence: int, kind: str, **event: object) -> dict[str, object]:
    return {
        "schema_version": 16,
        "run_id": "run-root",
        "event_id": f"event-{sequence}",
        "sequence": sequence,
        "occurred_at_unix_ms": sequence,
        "event": {"kind": kind, **event},
    }


def prepared(
    sequence: int,
    operation_id: str,
    call_id: str,
    name: str,
    parsed: object,
) -> dict[str, object]:
    return stored(
        sequence,
        "tool_prepared",
        operation_id=operation_id,
        invocation={
            "run_id": "run-root",
            "call_id": call_id,
            "name": name,
            "arguments": {"raw": "fixture", "parsed": parsed},
        },
        workspace_access="may_write",
    )


def started(sequence: int, operation_id: str) -> dict[str, object]:
    return stored(sequence, "tool_execution_started", operation_id=operation_id)


def committed(
    sequence: int,
    operation_id: str,
    call_id: str,
    name: str,
    *,
    failure_code: str | None = None,
    invocation: str = "accepted",
    transport: str = "succeeded",
    operation: str = "succeeded",
    side_effect: str = "applied",
    retry: str = "not_needed",
    revision: str = "a",
    legacy_revision: str | None = None,
) -> dict[str, object]:
    outcome = {
        "invocation": invocation,
        "transport": transport,
        "operation": operation,
        "side_effect": side_effect,
        "retry": retry,
        "evidence": {"status": "not_applicable"},
        "artifacts": [],
        "content": "fixture",
    }
    if failure_code is not None:
        outcome["failure_code"] = failure_code
    if legacy_revision is not None:
        outcome["workspace_revision"] = legacy_revision
    return stored(
        sequence,
        "tool_outcome_committed",
        operation_id=operation_id,
        call_id=call_id,
        name=name,
        outcome=outcome,
        workspace_state={
            "generation": sequence,
            "revision": {"status": "known", "sha256": revision * 64},
        },
    )


def failed(
    sequence: int,
    operation_id: str,
    call_id: str,
    name: str,
    failure_code: str,
    *,
    side_effect: str = "not_applied",
) -> dict[str, object]:
    return committed(
        sequence,
        operation_id,
        call_id,
        name,
        failure_code=failure_code,
        operation="failed",
        side_effect=side_effect,
        retry="after_correction" if side_effect == "not_applied" else "unsafe",
    )


class ObservationProjectionTests(unittest.TestCase):
    def assert_error(self, code: str, events: list[dict[str, object]]) -> None:
        with self.assertRaises(M7D.ObservationError) as raised:
            M7D.project_edit_observation(events)
        self.assertEqual(raised.exception.code, code)

    def test_requires_runtime_event_v16(self) -> None:
        events = [prepared(1, "op-1", "call-1", "edit_file", {"path": "a.txt"})]
        events[0]["schema_version"] = 15
        self.assert_error("runtime_event_schema_mismatch", events)

    def test_pairs_lifecycle_by_operation_id_not_call_id(self) -> None:
        events = [
            prepared(1, "op-1", "same-call", "edit_file", {"path": "a.txt"}),
            started(2, "op-2"),
            committed(3, "op-1", "same-call", "edit_file"),
        ]
        self.assert_error("tool_started_without_prepared", events)

    def test_unsuccessful_outcome_requires_failure_code(self) -> None:
        events = [
            prepared(1, "op-1", "call-1", "edit_file", {"path": "a.txt"}),
            committed(
                2,
                "op-1",
                "call-1",
                "edit_file",
                operation="failed",
                side_effect="not_applied",
                retry="after_correction",
            ),
        ]
        self.assert_error("unsuccessful_outcome_missing_failure_code", events)

    def test_preflight_failure_does_not_require_started_event(self) -> None:
        events = [
            prepared(1, "op-1", "call-1", "apply_patch", {"path": "a.txt"}),
            failed(2, "op-1", "call-1", "apply_patch", "patch_parse"),
        ]
        observation = M7D.project_edit_observation(events)
        self.assertEqual(observation["failure_codes"], {"patch_parse": 1})
        self.assertEqual(observation["incomplete_write_operations"], 0)

    def test_revision_comes_from_event_workspace_state(self) -> None:
        events = [
            prepared(1, "op-1", "call-1", "edit_file", {"path": "a.txt"}),
            started(2, "op-1"),
            committed(
                3,
                "op-1",
                "call-1",
                "edit_file",
                revision="b",
                legacy_revision="legacy-must-not-win",
            ),
        ]
        attempt = M7D.project_edit_observation(events)["edit_attempts"][0]
        self.assertEqual(
            attempt["workspace_revision"],
            {"status": "known", "sha256": "b" * 64},
        )

    def test_same_target_recovery_requires_a_later_model_request(self) -> None:
        no_model = [
            prepared(1, "op-1", "call-1", "edit_file", {"path": "a.txt"}),
            failed(2, "op-1", "call-1", "edit_file", "stale_read"),
            prepared(3, "op-2", "call-2", "edit_file", {"path": "a.txt"}),
            started(4, "op-2"),
            committed(5, "op-2", "call-2", "edit_file"),
        ]
        self.assertFalse(
            M7D.project_edit_observation(no_model)["edit_attempts"][0]["recovered"]
        )
        with_model = copy.deepcopy(no_model)
        with_model.insert(
            2,
            stored(
                3,
                "model_request_prepared",
                attempt_id="attempt-2",
                request={"request_number": 2},
            ),
        )
        for sequence, event in enumerate(with_model, start=1):
            event["sequence"] = sequence
            event["event_id"] = f"event-{sequence}"
            event["occurred_at_unix_ms"] = sequence
        first = M7D.project_edit_observation(with_model)["edit_attempts"][0]
        self.assertTrue(first["recovered"])
        self.assertEqual(first["model_requests_to_recovery"], 1)

    def test_different_target_or_tool_is_not_recovery(self) -> None:
        events = [
            prepared(1, "op-1", "call-1", "edit_file", {"path": "a.txt"}),
            failed(2, "op-1", "call-1", "edit_file", "ambiguous_edit"),
            stored(
                3,
                "model_request_prepared",
                attempt_id="attempt-2",
                request={"request_number": 2},
            ),
            prepared(4, "op-2", "call-2", "edit_file", {"path": "b.txt"}),
            started(5, "op-2"),
            committed(6, "op-2", "call-2", "edit_file"),
            prepared(7, "op-3", "call-3", "apply_patch", {"path": "a.txt"}),
            started(8, "op-3"),
            committed(9, "op-3", "call-3", "apply_patch"),
        ]
        first = M7D.project_edit_observation(events)["edit_attempts"][0]
        self.assertFalse(first["recovered"])

    def test_unresolved_patch_header_target_is_conservatively_unscorable(self) -> None:
        events = [
            prepared(
                1,
                "op-1",
                "call-1",
                "apply_patch",
                {"patch": "--- a.txt\n+++ a.txt\n"},
            ),
            failed(2, "op-1", "call-1", "apply_patch", "patch_parse"),
        ]
        attempt = M7D.project_edit_observation(events)["edit_attempts"][0]
        self.assertIsNone(attempt["target_identity_sha256"])
        self.assertEqual(
            M7D.project_edit_observation(events)["unscorable_recovery_failures"],
            1,
        )

    def test_any_indeterminate_edit_side_effect_is_transaction_ambiguity(self) -> None:
        events = [
            prepared(1, "op-1", "call-1", "apply_patch", {"path": "a.txt"}),
            started(2, "op-1"),
            failed(
                3,
                "op-1",
                "call-1",
                "apply_patch",
                "operation_failed",
                side_effect="indeterminate",
            ),
        ]
        observation = M7D.project_edit_observation(events)
        self.assertEqual(observation["transaction_ambiguities"], 1)

    def test_started_without_outcome_is_counted_not_guessed(self) -> None:
        events = [
            prepared(1, "op-1", "call-1", "apply_patch", {"path": "a.txt"}),
            started(2, "op-1"),
        ]
        observation = M7D.project_edit_observation(events)
        self.assertEqual(observation["incomplete_write_operations"], 1)
        self.assertEqual(observation["transaction_ambiguities"], 1)
        self.assertEqual(observation["edit_attempts"], [])

    def test_recovery_decision_does_not_use_global_unrelated_failures(self) -> None:
        records = [
            {
                "task_id": "t1",
                "observation": {
                    "failure_buckets": {"other_operation": 3},
                    "transaction_ambiguities": 0,
                },
            }
        ]
        decision = M7D.decide(records)
        self.assertEqual(decision["decision"], "no_edit_mechanism_admitted")

    def test_output_name_is_manifest_bound_and_non_replaceable(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            exact = root / M7D.MANIFEST["output"]["offline_result_name"]
            self.assertEqual(M7D.output_path(exact, root), exact.resolve())
            with self.assertRaises(M7D.ObservationError):
                M7D.output_path(root / "retry.json", root)
            exact.write_text("already exists", encoding="utf-8")
            with self.assertRaises(M7D.ObservationError):
                M7D.write_private_once(exact, {"status": "pass"})

    def test_manifest_has_no_live_or_credential_surface(self) -> None:
        encoded = M7D.canonical_bytes(M7D.MANIFEST)
        self.assertNotIn(b"key.txt", encoded)
        self.assertNotIn(b"credential_admission", encoded)
        self.assertEqual(M7D.MANIFEST["admission"]["official_api_requests"], 0)
        self.assertEqual(M7D.MANIFEST["admission"]["credential_read"], False)


if __name__ == "__main__":
    unittest.main(verbosity=2)
