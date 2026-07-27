def normalize_metadata(value):
    result = dict(value)
    result.setdefault("channel", "stable")
    return result
