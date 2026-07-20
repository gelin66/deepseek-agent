"""Profile composition utility used by the M6-B1 cross-file task."""

from __future__ import annotations

from typing import Any


def merge_profile(base: dict[str, Any], override: dict[str, Any]) -> dict[str, Any]:
    """Return an override of one agent profile without mutating either input."""
    result = dict(base)
    result.update(override)
    return result
