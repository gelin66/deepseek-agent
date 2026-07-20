"""Retry-delay utility with a deliberately failing boundary case."""


def retry_delay(attempt: int, base_seconds: float = 0.5, cap_seconds: float = 8.0) -> float:
    """Return a capped exponential delay for a zero-based retry attempt."""
    if attempt < 0:
        raise ValueError("attempt must be non-negative")
    return min(cap_seconds, base_seconds * (2 ** (attempt + 1)))
