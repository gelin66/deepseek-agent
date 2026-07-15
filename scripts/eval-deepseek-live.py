#!/usr/bin/env python3
"""Run a small, explicit-cost DeepSeek protocol canary.

The key is read from ``--key-file`` or ``DEEPSEEK_API_KEY`` and is never
written to argv or result records. Output is redacted JSONL suitable for
``eval/results``; model text and reasoning stay in memory only.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any, TextIO


SCHEMA = "codewhale.eval.deepseek-live.v1"
FLASH = "deepseek-v4-flash"
PRO = "deepseek-v4-pro"
STANDARD_URL = "https://api.deepseek.com/chat/completions"
STRICT_URL = "https://api.deepseek.com/beta/chat/completions"
FIM_URL = "https://api.deepseek.com/beta/completions"

REQUEST_CAP = 5
REQUEST_TIMEOUT_SECONDS = 45
SUITE_TIMEOUT_SECONDS = 180
MAX_RESPONSE_BYTES = 2 * 1024 * 1024
MAX_COST_USD = 0.01
PRICE_SNAPSHOT = "2026-07-15"
PRICES = {
    FLASH: {"hit": 0.0028, "miss": 0.14, "output": 0.28},
    PRO: {"hit": 0.003625, "miss": 0.435, "output": 0.87},
}
# Fixed prompts are tiny. This deliberately overstates their total input;
# output is hard-capped in each request below.
PLANNED_INPUT_BOUND = {FLASH: 16_000, PRO: 4_000}
PLANNED_OUTPUT_BOUND = {FLASH: 352, PRO: 64}


def emit(record: dict[str, Any], stream: TextIO = sys.stdout) -> None:
    record = {
        "schema": SCHEMA,
        "record_class": "protocol_canary",
        "product_metric_eligible": False,
        "verified_success": None,
        **record,
    }
    print(json.dumps(record, ensure_ascii=True, sort_keys=True), file=stream, flush=True)


def planned_cost_bound() -> float:
    total = 0.0
    for model in (FLASH, PRO):
        price = PRICES[model]
        total += PLANNED_INPUT_BOUND[model] * price["miss"] / 1_000_000
        total += PLANNED_OUTPUT_BOUND[model] * price["output"] / 1_000_000
    return total


def load_key(path: str | None) -> str:
    if path:
        raw = Path(path).read_bytes()
        if len(raw) > 4096:
            raise ValueError("key_too_large")
        key = raw.decode("utf-8").strip()
    else:
        key = os.environ.get("DEEPSEEK_API_KEY", "").strip()
    if not key:
        raise ValueError("missing_api_key")
    if any(character.isspace() or ord(character) < 32 for character in key):
        raise ValueError("invalid_key_format")
    return key


def function_tool(
    name: str,
    properties: dict[str, Any],
    required: list[str],
    *,
    strict: bool = False,
) -> dict[str, Any]:
    function: dict[str, Any] = {
        "name": name,
        "description": f"DeepSeek protocol canary function {name}.",
        "parameters": {
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": False,
        },
    }
    if strict:
        function["strict"] = True
    return {"type": "function", "function": function}


def curl_config(key: str) -> bytes:
    escaped = key.replace("\\", "\\\\").replace('"', '\\"')
    return (
        f'header = "Authorization: Bearer {escaped}"\n'
        'header = "Content-Type: application/json"\n'
        'header = "Accept: application/json"\n'
    ).encode()


def post_json(
    curl: str,
    key: str,
    url: str,
    body: dict[str, Any],
    deadline: float,
) -> dict[str, Any]:
    started = time.monotonic()
    remaining = deadline - started
    if remaining <= 0:
        return {"attempted": False, "error": "suite_timeout", "seconds": 0.0}
    timeout = min(float(REQUEST_TIMEOUT_SECONDS), remaining)
    work = tempfile.mkdtemp(prefix="codewhale-deepseek-canary-")
    os.chmod(work, 0o700)
    request_path = Path(work, "request.json")
    try:
        request_path.write_text(json.dumps(body, ensure_ascii=False), encoding="utf-8")
        os.chmod(request_path, 0o600)
        command = [
            curl,
            "--disable",
            "--config",
            "-",
            "--silent",
            "--request",
            "POST",
            "--url",
            url,
            "--data-binary",
            f"@{request_path}",
            "--write-out",
            "\n%{http_code}",
            "--connect-timeout",
            str(min(15, int(timeout))),
            "--max-time",
            str(max(1, int(timeout))),
            "--max-filesize",
            str(MAX_RESPONSE_BYTES),
            "--proto",
            "=https",
        ]
        environment = os.environ.copy()
        environment.pop("DEEPSEEK_API_KEY", None)
        completed = subprocess.run(
            command,
            input=curl_config(key),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=timeout + 1,
            check=False,
            env=environment,
        )
        raw, separator, status_bytes = completed.stdout.rpartition(b"\n")
        status_text = status_bytes.decode("ascii", "ignore").strip() if separator else ""
        status = int(status_text) if status_text.isdigit() else None
        elapsed = time.monotonic() - started
        if completed.returncode != 0:
            error = "request_timeout" if completed.returncode == 28 else "curl_error"
            return {"attempted": True, "http_status": status, "error": error, "seconds": elapsed}
        if status is None:
            return {"attempted": True, "error": "missing_http_status", "seconds": elapsed}
        if not 200 <= status < 300:
            return {
                "attempted": True,
                "http_status": status,
                "error": f"http_{status}",
                "seconds": elapsed,
            }
        if len(raw) > MAX_RESPONSE_BYTES:
            return {"attempted": True, "http_status": status, "error": "response_too_large", "seconds": elapsed}
        try:
            payload = json.loads(raw)
        except (UnicodeDecodeError, json.JSONDecodeError):
            return {"attempted": True, "http_status": status, "error": "invalid_json", "seconds": elapsed}
        if not isinstance(payload, dict):
            return {"attempted": True, "http_status": status, "error": "invalid_shape", "seconds": elapsed}
        return {
            "attempted": True,
            "http_status": status,
            "payload": payload,
            "error": None,
            "seconds": elapsed,
        }
    except subprocess.TimeoutExpired:
        return {"attempted": True, "error": "request_timeout", "seconds": time.monotonic() - started}
    except OSError:
        return {"attempted": False, "error": "local_io_error", "seconds": time.monotonic() - started}
    finally:
        shutil.rmtree(work, ignore_errors=True)


def choice(outcome: dict[str, Any]) -> dict[str, Any]:
    payload = outcome.get("payload")
    choices = payload.get("choices") if isinstance(payload, dict) else None
    if isinstance(choices, list) and choices and isinstance(choices[0], dict):
        return choices[0]
    return {}


def message(outcome: dict[str, Any]) -> dict[str, Any]:
    value = choice(outcome).get("message")
    return value if isinstance(value, dict) else {}


def tool_calls(outcome: dict[str, Any]) -> list[dict[str, Any]]:
    value = message(outcome).get("tool_calls")
    if not isinstance(value, list):
        return []
    return [call for call in value if isinstance(call, dict)]


def arguments(call: dict[str, Any]) -> dict[str, Any] | None:
    function = call.get("function")
    raw = function.get("arguments") if isinstance(function, dict) else None
    if not isinstance(raw, str):
        return None
    try:
        parsed = json.loads(raw)
    except json.JSONDecodeError:
        return None
    return parsed if isinstance(parsed, dict) else None


def function_name(call: dict[str, Any]) -> str | None:
    function = call.get("function")
    name = function.get("name") if isinstance(function, dict) else None
    return name if isinstance(name, str) else None


def usage(outcome: dict[str, Any]) -> dict[str, int | None]:
    payload = outcome.get("payload")
    raw = payload.get("usage") if isinstance(payload, dict) else None
    raw = raw if isinstance(raw, dict) else {}
    details = raw.get("completion_tokens_details")
    details = details if isinstance(details, dict) else {}

    def number(name: str, source: dict[str, Any] = raw) -> int | None:
        value = source.get(name)
        return value if type(value) is int and value >= 0 else None

    return {
        "prompt_tokens": number("prompt_tokens"),
        "completion_tokens": number("completion_tokens"),
        "total_tokens": number("total_tokens"),
        "cache_hit_tokens": number("prompt_cache_hit_tokens"),
        "cache_miss_tokens": number("prompt_cache_miss_tokens"),
        "reasoning_tokens": number("reasoning_tokens", details),
    }


def usage_complete(outcome: dict[str, Any]) -> bool:
    measured = usage(outcome)
    prompt = measured["prompt_tokens"]
    completion = measured["completion_tokens"]
    total = measured["total_tokens"]
    hit = measured["cache_hit_tokens"]
    miss = measured["cache_miss_tokens"]
    return (
        None not in (prompt, completion, total, hit, miss)
        and prompt == hit + miss
        and total == prompt + completion
    )


def estimated_cost(model: str, measured: dict[str, int | None]) -> float | None:
    prompt = measured["prompt_tokens"]
    output = measured["completion_tokens"]
    hit = measured["cache_hit_tokens"]
    miss = measured["cache_miss_tokens"]
    if None in (prompt, output, hit, miss):
        return None
    price = PRICES[model]
    return (hit * price["hit"] + miss * price["miss"] + output * price["output"]) / 1_000_000


def record(
    case_id: str,
    step: str,
    surface: str,
    model: str,
    outcome: dict[str, Any],
    assertions: dict[str, bool],
) -> tuple[bool, float | None]:
    measured = usage(outcome)
    cost = estimated_cost(model, measured)
    passed = outcome.get("error") is None and bool(assertions) and all(assertions.values())
    emit(
        {
            "record_type": "request",
            "case_id": case_id,
            "step": step,
            "surface": surface,
            "model": model,
            "request_attempted": bool(outcome.get("attempted")),
            "http_status": outcome.get("http_status"),
            "finish_reason": choice(outcome).get("finish_reason"),
            "usage": measured,
            "estimated_cost_usd": round(cost, 9) if cost is not None else None,
            "price_snapshot": PRICE_SNAPSHOT,
            "assertions": assertions,
            "request_duration_seconds": round(float(outcome.get("seconds", 0.0)), 3),
            "status": "passed" if passed else "failed",
            "error_code": outcome.get("error"),
        }
    )
    return passed, cost


def git_metadata() -> dict[str, Any]:
    root = Path(__file__).resolve().parent.parent
    try:
        revision = subprocess.run(
            ["git", "rev-parse", "HEAD"], cwd=root, text=True, capture_output=True, check=True
        ).stdout.strip()
        dirty = bool(
            subprocess.run(
                ["git", "status", "--porcelain"], cwd=root, text=True, capture_output=True, check=True
            ).stdout
        )
        return {"revision": revision, "dirty": dirty}
    except (OSError, subprocess.CalledProcessError):
        return {"revision": None, "dirty": None}


def run_live(curl: str, key: str) -> int:
    started = time.monotonic()
    deadline = started + SUITE_TIMEOUT_SECONDS
    attempted = 0
    results: list[bool] = []
    total_cost = 0.0
    cost_known = True

    def run(
        case_id: str,
        step: str,
        surface: str,
        model: str,
        url: str,
        body: dict[str, Any],
        check: Any,
    ) -> dict[str, Any]:
        nonlocal attempted, cost_known, total_cost
        if attempted >= REQUEST_CAP:
            outcome = {"attempted": False, "error": "request_cap", "seconds": 0.0}
        elif total_cost >= MAX_COST_USD:
            outcome = {"attempted": False, "error": "cost_cap", "seconds": 0.0}
        else:
            outcome = post_json(curl, key, url, body, deadline)
            attempted += int(bool(outcome.get("attempted")))
        passed, cost = record(case_id, step, surface, model, outcome, check(outcome))
        results.append(passed)
        if cost is None:
            cost_known = False
        else:
            total_cost += cost
        return outcome

    standard = {
        "model": FLASH,
        "messages": [{"role": "user", "content": "Return one short non-empty canary response."}],
        "thinking": {"type": "disabled"},
        "max_tokens": 32,
        "stream": False,
    }
    run(
        "standard_chat_non_thinking",
        "completion",
        "standard_chat",
        FLASH,
        STANDARD_URL,
        standard,
        lambda out: {
            "http_200": out.get("http_status") == 200,
            "finish_stop": choice(out).get("finish_reason") == "stop",
            "content_nonempty": bool(message(out).get("content")),
            "reasoning_empty": not message(out).get("reasoning_content"),
            "usage_complete": usage_complete(out),
        },
    )

    echo = function_tool("canary_echo", {"value": {"type": "string"}}, ["value"])
    thinking = {
        "model": FLASH,
        "messages": [{
            "role": "user",
            "content": "Call canary_echo once with value CANARY. After its result, answer with that result only.",
        }],
        "thinking": {"type": "enabled"},
        "reasoning_effort": "high",
        "tools": [echo],
        # DeepSeek thinking mode rejects explicit tool_choice.
        "max_tokens": 128,
        "stream": False,
    }
    first = run(
        "thinking_tool_replay",
        "tool_call",
        "standard_chat",
        FLASH,
        STANDARD_URL,
        thinking,
        lambda out: {
            "http_200": out.get("http_status") == 200,
            "finish_tool_calls": choice(out).get("finish_reason") == "tool_calls",
            "reasoning_nonempty": bool(message(out).get("reasoning_content")),
            "one_tool_call": len(tool_calls(out)) == 1,
            "tool_id_present": len(tool_calls(out)) == 1 and bool(tool_calls(out)[0].get("id")),
            "tool_name_matches": len(tool_calls(out)) == 1
            and function_name(tool_calls(out)[0]) == "canary_echo",
            "arguments_match": len(tool_calls(out)) == 1
            and arguments(tool_calls(out)[0]) == {"value": "CANARY"},
            "usage_complete": usage_complete(out),
        },
    )
    assistant = message(first)
    calls = tool_calls(first)
    if calls and isinstance(calls[0], dict) and assistant.get("reasoning_content"):
        replay = {
            **thinking,
            "messages": [
                thinking["messages"][0],
                {name: assistant.get(name) for name in ("role", "content", "reasoning_content", "tool_calls")},
                {"role": "tool", "tool_call_id": calls[0].get("id"), "content": "REPLAY_OK"},
            ],
        }
        run(
            "thinking_tool_replay",
            "exact_replay",
            "standard_chat",
            FLASH,
            STANDARD_URL,
            replay,
            lambda out: {
                "http_200": out.get("http_status") == 200,
                "finish_stop": choice(out).get("finish_reason") == "stop",
                "content_matches_tool_result": str(message(out).get("content") or "").strip()
                == "REPLAY_OK",
                "usage_complete": usage_complete(out),
            },
        )
    else:
        skipped = {"attempted": False, "error": "replay_prerequisite", "seconds": 0.0}
        passed, cost = record(
            "thinking_tool_replay", "exact_replay", "standard_chat", FLASH, skipped,
            {"prior_tool_call_valid": False},
        )
        results.append(passed)
        if cost is None:
            cost_known = False
        else:
            total_cost += cost

    strict_tools = [
        function_tool("canary_flag", {"enabled": {"type": "boolean"}}, ["enabled"], strict=True),
        function_tool("canary_note", {"note": {"type": "string"}}, ["note"], strict=True),
    ]
    strict = {
        "model": FLASH,
        "messages": [{"role": "user", "content": "Call canary_flag with enabled true."}],
        "thinking": {"type": "disabled"},
        "tools": strict_tools,
        "tool_choice": {"type": "function", "function": {"name": "canary_flag"}},
        "max_tokens": 64,
        "stream": False,
    }
    run(
        "beta_strict_catalog",
        "tool_call",
        "strict_chat",
        FLASH,
        STRICT_URL,
        strict,
        lambda out: {
            "http_200": out.get("http_status") == 200,
            "all_functions_strict": all(tool["function"].get("strict") is True for tool in strict_tools),
            "finish_tool_calls": choice(out).get("finish_reason") == "tool_calls",
            "one_tool_call": len(tool_calls(out)) == 1,
            "tool_id_present": len(tool_calls(out)) == 1 and bool(tool_calls(out)[0].get("id")),
            "tool_type_function": len(tool_calls(out)) == 1
            and tool_calls(out)[0].get("type") == "function",
            "tool_name_matches": len(tool_calls(out)) == 1
            and function_name(tool_calls(out)[0]) == "canary_flag",
            "strict_arguments_valid": len(tool_calls(out)) == 1
            and arguments(tool_calls(out)[0]) == {"enabled": True},
            "usage_complete": usage_complete(out),
        },
    )

    fim = {
        "model": PRO,
        "prompt": "def deepseek_canary() -> str:\n    return ",
        "suffix": "\n",
        "max_tokens": 64,
    }
    run(
        "beta_fim",
        "completion",
        "fim",
        PRO,
        FIM_URL,
        fim,
        lambda out: {
            "http_200": out.get("http_status") == 200,
            "finish_stop": choice(out).get("finish_reason") == "stop",
            "text_nonempty": bool(choice(out).get("text")),
            "usage_complete": usage_complete(out),
        },
    )

    passed = sum(results)
    suite_passed = (
        len(results) == REQUEST_CAP
        and passed == len(results)
        and cost_known
        and total_cost <= MAX_COST_USD
    )
    emit(
        {
            "record_type": "summary",
            "status": "passed" if suite_passed else "failed",
            "planned_requests": REQUEST_CAP,
            "requests_attempted": attempted,
            "records_total": len(results),
            "records_passed": passed,
            "records_failed": len(results) - passed,
            "suite_duration_seconds": round(time.monotonic() - started, 3),
            "estimated_cost_usd": round(total_cost, 9),
            "cost_known": cost_known,
            "cost_within_cap": cost_known and total_cost <= MAX_COST_USD,
            "max_cost_usd": MAX_COST_USD,
            "price_snapshot": PRICE_SNAPSHOT,
            **git_metadata(),
        }
    )
    return 0 if suite_passed else 1


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--acknowledge-cost", action="store_true")
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--key-file")
    args = parser.parse_args()

    bound = planned_cost_bound()
    if bound > MAX_COST_USD:
        emit({"record_type": "error", "error_code": "planned_cost_exceeds_cap"}, sys.stderr)
        return 2
    if args.dry_run:
        emit({
            "record_type": "plan",
            "request_cap": REQUEST_CAP,
            "request_timeout_seconds": REQUEST_TIMEOUT_SECONDS,
            "suite_timeout_seconds": SUITE_TIMEOUT_SECONDS,
            "planned_cost_bound_usd": round(bound, 9),
            "max_cost_usd": MAX_COST_USD,
            "key_accessed": False,
            "network_accessed": False,
            "cases": ["standard_chat", "thinking_tool_replay", "strict_chat", "fim"],
        })
        return 0
    if not args.acknowledge_cost:
        emit({
            "record_type": "error",
            "error_code": "cost_acknowledgement_required",
            "key_accessed": False,
            "network_accessed": False,
        }, sys.stderr)
        return 2
    curl = shutil.which("curl")
    if not curl:
        emit({"record_type": "error", "error_code": "curl_unavailable"}, sys.stderr)
        return 2
    try:
        key = load_key(args.key_file)
    except (OSError, UnicodeDecodeError, ValueError) as error:
        code = str(error) if isinstance(error, ValueError) else "key_unreadable"
        emit({"record_type": "error", "error_code": code, "network_accessed": False}, sys.stderr)
        return 2
    try:
        return run_live(curl, key)
    finally:
        key = ""


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except KeyboardInterrupt:
        emit({"record_type": "error", "error_code": "interrupted"}, sys.stderr)
        raise SystemExit(130) from None
