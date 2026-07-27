from typing import Any

from ledger import summarize_attempts


def encode_summary(records: list[dict[str, Any]]) -> dict[str, Any]:
    return {"version": 1, "summary": summarize_attempts(records)}
