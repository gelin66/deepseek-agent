def may_export(
    requested_account: str, authenticated_account: str, is_admin: bool
) -> bool:
    return bool(requested_account)
