"""Merge a base Agent policy with a task-specific override."""


def merge_policy(base: dict, override: dict) -> dict:
    """Merge policy dictionaries without mutating either input."""
    merged = dict(base)
    merged.update(override)
    return merged
