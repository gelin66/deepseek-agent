#!/usr/bin/env python3
"""Run the credential-free M22 canonical streaming-delta benchmark."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import stat
import statistics
import subprocess
import sys
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
MANIFEST_PATH = ROOT / "eval/manifests/m22-streaming-delta-baseline-v1.json"
FIXTURE_PATH = ROOT / "eval/fixtures/m22-streaming-delta-baseline-v1.json"
MARKER = "M22_BENCHMARK_JSON="
WALL_METRICS = (
    "delta_append_us",
    "credential_free_reopen_us",
    "run_api_events_json_us",
    "tui_projection_us",
    "headless_projection_us",
)
WALL_THRESHOLD_KEYS = {
    "delta_append_us": "delta_append_wall_ms_median",
    "credential_free_reopen_us": "credential_free_reopen_wall_ms_median",
    "run_api_events_json_us": "run_api_events_json_wall_ms_median",
    "tui_projection_us": "tui_projection_wall_ms_median",
    "headless_projection_us": "headless_projection_wall_ms_median",
}
BYTE_METRIC = "sqlite_plus_wal_bytes"


class EvaluationError(RuntimeError):
    pass


def require(condition: bool, code: str) -> None:
    if not condition:
        raise EvaluationError(code)


def load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise EvaluationError(f"invalid_json:{path.relative_to(ROOT)}") from error
    require(isinstance(value, dict), f"json_root_not_object:{path.relative_to(ROOT)}")
    return value


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def git(*arguments: str) -> str:
    completed = subprocess.run(
        ["git", *arguments],
        cwd=ROOT,
        check=True,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    return completed.stdout.strip()


def validate_contract() -> tuple[dict[str, Any], dict[str, Any]]:
    manifest = load_json(MANIFEST_PATH)
    fixture = load_json(FIXTURE_PATH)
    require(
        manifest.get("schema") == "dse.eval.m22-streaming-delta-baseline.v1",
        "manifest_schema_invalid",
    )
    require(
        fixture.get("schema")
        == "dse.eval.m22-streaming-delta-baseline-fixture.v1",
        "fixture_schema_invalid",
    )
    benchmark = manifest.get("benchmark", {})
    require(benchmark.get("maximum_reruns") == 0, "maximum_reruns_invalid")
    require(benchmark.get("credential_read") is False, "credential_contract_invalid")
    require(benchmark.get("official_api_requests") == 0, "api_contract_invalid")
    require(benchmark.get("external_network") is False, "network_contract_invalid")
    require(
        benchmark.get("warmups_per_profile") == 1
        and benchmark.get("measured_repetitions_per_profile") == 5,
        "repetition_contract_invalid",
    )
    profiles = fixture.get("profiles")
    require(isinstance(profiles, list) and len(profiles) == 2, "profile_count_invalid")
    for profile in profiles:
        reasoning = profile["observed_reasoning_delta_events"]
        content = profile["observed_content_delta_events"]
        require(
            profile["synthetic_total_events"] == reasoning + content + 3,
            f"synthetic_event_count_invalid:{profile['id']}",
        )
        require(
            profile["observed_reasoning_delta_utf8_bytes"] >= reasoning
            and profile["observed_content_delta_utf8_bytes"] >= content,
            f"delta_byte_contract_invalid:{profile['id']}",
        )

    source = fixture["source"]
    raw = ROOT / source["journal"]
    require(raw.is_file() and not raw.is_symlink(), "m20b_raw_missing")
    mode = stat.S_IMODE(raw.stat().st_mode)
    require(mode == 0o600, "m20b_raw_mode_invalid")
    require(raw.stat().st_size == source["journal_bytes"], "m20b_raw_size_invalid")
    require(
        f"sha256:{sha256(raw)}" == source["journal_sha256"],
        "m20b_raw_hash_invalid",
    )
    return manifest, fixture


def aggregate_profile(
    profile: dict[str, Any], thresholds: dict[str, Any]
) -> dict[str, Any]:
    samples = profile["samples"]
    require(len(samples) == 5, f"sample_count_invalid:{profile['id']}")
    metrics: dict[str, Any] = {}
    for metric in (*WALL_METRICS, BYTE_METRIC):
        values = [int(sample[metric]) for sample in samples]
        require(all(value >= 0 for value in values), f"negative_metric:{metric}")
        median = int(statistics.median(values))
        metrics[metric] = {
            "samples": values,
            "minimum": min(values),
            "median": median,
            "maximum": max(values),
        }

    observed_total = (
        profile["reasoning_delta_events"] + profile["content_delta_events"]
    )
    delta_share = observed_total / profile["synthetic_total_events"]
    minimum_share = float(thresholds["delta_event_share_minimum"])
    absolute = thresholds["absolute_any"]
    triggers = []
    for metric in WALL_METRICS:
        threshold_key = WALL_THRESHOLD_KEYS[metric]
        threshold_us = int(absolute[threshold_key]) * 1000
        measured = metrics[metric]
        stable = measured["maximum"] <= measured["median"] * 1.5
        if measured["median"] >= threshold_us and stable:
            triggers.append(
                {
                    "metric": metric,
                    "median": measured["median"],
                    "threshold": threshold_us,
                    "stable": True,
                }
            )
    byte_threshold = int(absolute["sqlite_plus_wal_bytes_median"])
    if metrics[BYTE_METRIC]["median"] >= byte_threshold:
        triggers.append(
            {
                "metric": BYTE_METRIC,
                "median": metrics[BYTE_METRIC]["median"],
                "threshold": byte_threshold,
                "stable": True,
            }
        )
    return {
        "id": profile["id"],
        "synthetic_total_events": profile["synthetic_total_events"],
        "delta_event_share": delta_share,
        "metrics": metrics,
        "canonical_json_bytes": [
            int(sample["canonical_json_bytes"]) for sample in samples
        ],
        "projected_effects": [
            int(sample["projected_effects"]) for sample in samples
        ],
        "headless_bytes": [int(sample["headless_bytes"]) for sample in samples],
        "material_triggers": triggers if delta_share >= minimum_share else [],
    }


def benchmark(output: Path) -> dict[str, Any]:
    manifest, fixture = validate_contract()
    require(git("branch", "--show-current") == "deepseek-agent", "branch_invalid")
    baseline = manifest["baseline"]["commit"]
    subprocess.run(
        ["git", "merge-base", "--is-ancestor", baseline, "HEAD"],
        cwd=ROOT,
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    require(not output.exists(), "output_exists")
    ignored = subprocess.run(
        ["git", "check-ignore", "-q", str(output)],
        cwd=ROOT,
        check=False,
    )
    require(ignored.returncode == 0, "output_not_ignored")

    environment = os.environ.copy()
    environment.update(
        {
            "CARGO_INCREMENTAL": "0",
            "CARGO_NET_OFFLINE": "true",
            "CARGO_TARGET_DIR": "/private/tmp/dse-m22-target",
        }
    )
    command = [
        "cargo",
        "test",
        "-p",
        "dse-tui",
        "--bin",
        "dse-tui",
        "m22_streaming_delta_benchmark::canonical_m22_streaming_delta_baseline",
        "--locked",
        "--",
        "--ignored",
        "--nocapture",
        "--test-threads=1",
    ]
    completed = subprocess.run(
        command,
        cwd=ROOT,
        env=environment,
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    if completed.returncode != 0:
        sys.stderr.write(completed.stdout)
        raise EvaluationError("rust_benchmark_failed")
    lines = [
        line.split(MARKER, maxsplit=1)[1]
        for line in completed.stdout.splitlines()
        if MARKER in line
    ]
    require(len(lines) == 1, "benchmark_marker_invalid")
    rust_report = json.loads(lines[0])
    require(
        rust_report.get("schema")
        == "dse.eval.m22-streaming-delta-rust-benchmark.v1",
        "rust_report_schema_invalid",
    )
    require(
        rust_report.get("warmups_per_profile") == 1
        and rust_report.get("measured_repetitions_per_profile") == 5,
        "rust_report_repetitions_invalid",
    )
    fixture_by_id = {profile["id"]: profile for profile in fixture["profiles"]}
    require(
        {profile["id"] for profile in rust_report["profiles"]}
        == set(fixture_by_id),
        "rust_report_profiles_invalid",
    )
    for profile in rust_report["profiles"]:
        expected = fixture_by_id[profile["id"]]
        require(
            profile["reasoning_delta_events"]
            == expected["observed_reasoning_delta_events"]
            and profile["reasoning_delta_utf8_bytes"]
            == expected["observed_reasoning_delta_utf8_bytes"]
            and profile["content_delta_events"]
            == expected["observed_content_delta_events"]
            and profile["content_delta_utf8_bytes"]
            == expected["observed_content_delta_utf8_bytes"]
            and profile["synthetic_total_events"]
            == expected["synthetic_total_events"],
            f"rust_profile_identity_invalid:{profile['id']}",
        )

    profiles = [
        aggregate_profile(profile, manifest["material_loss_gate"])
        for profile in rust_report["profiles"]
    ]
    material = any(profile["material_triggers"] for profile in profiles)
    result = {
        "schema": "dse.eval.m22-streaming-delta-baseline-result.v1",
        "suite_id": manifest["suite_id"],
        "source_identity": {
            "commit": git("rev-parse", "HEAD"),
            "tree": git("rev-parse", "HEAD^{tree}"),
            "manifest_sha256": f"sha256:{sha256(MANIFEST_PATH)}",
            "fixture_sha256": f"sha256:{sha256(FIXTURE_PATH)}",
            "harness_sha256": f"sha256:{sha256(Path(__file__))}",
        },
        "credential_read": False,
        "official_api_requests": 0,
        "external_network": False,
        "maximum_reruns": 0,
        "profiles": profiles,
        "baseline_material_loss": material,
        "decision": (
            "baseline_material_loss_observed_candidate_audit_required"
            if material
            else "reject_no_material_production_streaming_delta_loss"
        ),
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    descriptor = os.open(
        output,
        os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0),
        0o600,
    )
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as handle:
            json.dump(result, handle, ensure_ascii=False, sort_keys=True, indent=2)
            handle.write("\n")
            handle.flush()
            os.fsync(handle.fileno())
    except Exception:
        output.unlink(missing_ok=True)
        raise
    require(stat.S_IMODE(output.stat().st_mode) == 0o600, "output_mode_invalid")
    return result


def self_test() -> None:
    manifest, fixture = validate_contract()
    require(len(fixture["profiles"]) == 2, "self_test_profiles_invalid")
    thresholds = manifest["material_loss_gate"]
    synthetic = {
        "id": "self-test",
        "reasoning_delta_events": 90,
        "content_delta_events": 10,
        "synthetic_total_events": 103,
        "samples": [
            {
                "delta_append_us": value,
                "credential_free_reopen_us": 10,
                "run_api_events_json_us": 10,
                "tui_projection_us": 10,
                "headless_projection_us": 10,
                "sqlite_plus_wal_bytes": 1,
                "canonical_json_bytes": 1,
                "projected_effects": 103,
                "headless_bytes": 1,
            }
            for value in (1_000_000, 1_010_000, 1_020_000, 1_030_000, 1_040_000)
        ],
    }
    report = aggregate_profile(synthetic, thresholds)
    require(
        report["material_triggers"]
        and report["material_triggers"][0]["metric"] == "delta_append_us",
        "self_test_material_gate_invalid",
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    subcommands = parser.add_subparsers(dest="command", required=True)
    subcommands.add_parser("self-test")
    run = subcommands.add_parser("run")
    run.add_argument("--output", type=Path, required=True)
    arguments = parser.parse_args()
    try:
        if arguments.command == "self-test":
            self_test()
            print("M22 self-test passed")
            return 0
        result = benchmark(arguments.output.resolve())
        print(json.dumps(result, ensure_ascii=False, sort_keys=True))
        return 0
    except (EvaluationError, subprocess.CalledProcessError) as error:
        print(f"M22 evaluation failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
