"""Retry delay calculation with a zero-based attempt index."""


def retry_delay(attempt: int, base: int = 2, cap: int = 30) -> int:
    if attempt < 0:
        raise ValueError("attempt must be non-negative")
    return min(cap, base ** (attempt + 1))
