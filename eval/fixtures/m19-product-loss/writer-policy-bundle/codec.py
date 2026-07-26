from validation import normalize_scopes


def decode_policy(value):
    if not isinstance(value, dict):
        raise ValueError("policy must be an object")
    return {
        "version": int(value.get("version", 1)),
        "model": str(value.get("model", "")),
        "scopes": normalize_scopes(value.get("scopes", [])),
    }
