import json

from config import normalize_shard_config


def encode_config(value):
    return json.dumps(normalize_shard_config(value))
