from typing import Any


def summarize_attempts(records: list[dict[str, Any]]) -> dict[str, Any]:
    completed = [record for record in records if record.get("status") == "completed"]
    return {
        "physical_started": len(records),
        "physical_completed": len(completed),
        "physical_in_flight": len(records) - len(completed),
        "logical_requests": len({record.get("logical_id") for record in records}),
        "usage_complete": len(records) == len(completed),
        "input_tokens": sum(int(record.get("input_tokens") or 0) for record in completed),
        "output_tokens": sum(int(record.get("output_tokens") or 0) for record in completed),
        "cost_nanousd": sum(int(record.get("cost_nanousd") or 0) for record in completed),
    }
