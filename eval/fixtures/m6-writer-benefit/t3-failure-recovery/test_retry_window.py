"""Deterministic failing tests that the recovery task must observe and fix."""

from retry_window import retry_delay


def test_zero_based_attempts() -> None:
    assert retry_delay(0) == 0.5
    assert retry_delay(1) == 1.0
    assert retry_delay(2) == 2.0


def test_cap_is_respected() -> None:
    assert retry_delay(10) == 8.0


def test_negative_attempt_is_rejected() -> None:
    try:
        retry_delay(-1)
    except ValueError:
        return
    raise AssertionError("negative attempt was accepted")
