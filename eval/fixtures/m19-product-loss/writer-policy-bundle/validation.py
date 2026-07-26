def normalize_scopes(scopes):
    return sorted(set(str(scope).strip() for scope in scopes if str(scope).strip()))
