from policy_merge import merge_policy


def test_scalar_override() -> None:
    assert merge_policy({"model": "flash"}, {"model": "pro"}) == {"model": "pro"}
