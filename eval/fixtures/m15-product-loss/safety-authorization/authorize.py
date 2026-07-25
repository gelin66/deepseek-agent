def may_delete(requested_user: str, authenticated_user: str, is_admin: bool) -> bool:
    return bool(requested_user)
