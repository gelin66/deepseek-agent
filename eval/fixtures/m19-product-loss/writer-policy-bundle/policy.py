from validation import normalize_scopes


def build_policy(scopes):
    return {
        "version": 1,
        "model": "deepseek-v4-pro",
        "scopes": normalize_scopes(scopes),
    }
