def parse_scope_token(value):
    if not isinstance(value, str):
        return None
    parts = [part.strip().lower() for part in value.split(":")]
    if len(parts) != 2 or not all(parts):
        return None
    return tuple(parts)
