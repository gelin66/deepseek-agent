"""Small deterministic slug helper used by the M7 evaluation fixture."""


def slugify(value: str) -> str:
    """Return a lowercase ASCII slug made from letters and digits."""
    normalized = value.strip().lower().replace(" ", "-")
    return "".join(character for character in normalized if character.isalnum() or character == "-")
