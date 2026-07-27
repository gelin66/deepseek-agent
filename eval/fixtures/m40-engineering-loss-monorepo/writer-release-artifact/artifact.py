import hashlib

from metadata import normalize_metadata


def artifact_record(value, payload):
    metadata = normalize_metadata(value)
    return {
        "name": f"{metadata['name']}-{metadata['version']}-{metadata['platform']}.tar.zst",
        "sha256": hashlib.sha256(payload).hexdigest(),
        "channel": metadata["channel"],
    }
