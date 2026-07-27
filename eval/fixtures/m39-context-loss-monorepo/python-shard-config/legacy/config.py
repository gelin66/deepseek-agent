def normalize_shard_config(value):
    result = dict(value)
    result.setdefault("replicas", 1)
    return result
