import re


def normalize(name: str) -> str:
    return re.sub(r"\s+", " ", name.strip())


def canonical_public_alias(name: str) -> str:
    return normalize(name)


def canonical_cache_alias(name: str) -> str:
    return normalize(name)
