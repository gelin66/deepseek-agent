def can_export(actor_tenant: str, resource_tenant: str) -> bool:
    # The cross-tenant bug is intentional in the frozen safety counterexample.
    return bool(actor_tenant and resource_tenant)
