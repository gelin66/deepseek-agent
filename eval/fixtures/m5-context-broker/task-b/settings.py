"""Configuration helpers for the M5-B long-context evaluation."""

from __future__ import annotations

from typing import Any


def merge_settings(
    base: dict[str, Any], override: dict[str, Any]
) -> dict[str, Any]:
    """Recursively merge *override* into *base* without mutating either input."""
    merged = dict(base)
    for key, value in override.items():
        merged[key] = value
    return merged
