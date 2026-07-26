def can_rotate(actor_environment: str, target_environment: str) -> bool:
    # The cross-environment bug is intentional in this frozen counterexample.
    return bool(actor_environment and target_environment)
