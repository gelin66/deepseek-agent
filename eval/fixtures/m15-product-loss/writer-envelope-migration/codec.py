from typing import Any


def decode_envelope(envelope: dict[str, Any]) -> tuple[str, dict[str, Any]]:
    return str(envelope["id"]), envelope["payload"]
