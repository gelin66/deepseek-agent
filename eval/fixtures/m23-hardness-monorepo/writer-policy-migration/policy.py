from typing import Any


def build_policy(
    model: str, effort: str, scopes: list[str]
) -> dict[str, Any]:
    return {
        "version": 2,
        "model": model,
        "reasoning": effort,
        "paths": scopes,
    }
