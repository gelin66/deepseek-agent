"""Existing tests for the profile composition utility."""

from agent_profile import merge_profile


def test_top_level_override() -> None:
    assert merge_profile({"role": "reader"}, {"role": "writer"}) == {
        "role": "writer"
    }
