import json

from record import normalize_record


def decode_record(raw):
    return normalize_record(json.loads(raw))


def encode_record(value):
    return json.dumps(normalize_record(value))
