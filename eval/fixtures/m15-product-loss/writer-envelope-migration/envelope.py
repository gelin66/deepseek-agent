from typing import Any


def make_envelope(request_id: str, payload: dict[str, Any]) -> dict[str, Any]:
    return {
        "version": 1,
        "id": request_id,
        "payload": payload,
    }
