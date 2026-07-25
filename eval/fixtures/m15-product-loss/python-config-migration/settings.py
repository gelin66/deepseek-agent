from typing import Any


def load_settings(payload: dict[str, Any]) -> dict[str, Any]:
    result = dict(payload)
    timeout = result.get("request_timeout_ms", 30_000)
    result["request_timeout_ms"] = int(timeout)
    return result
