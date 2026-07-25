from typing import Any


def decode_policy(payload: dict[str, Any]) -> tuple[str, str, list[str]]:
    return (
        str(payload["model"]),
        str(payload["reasoning"]),
        list(payload["paths"]),
    )
