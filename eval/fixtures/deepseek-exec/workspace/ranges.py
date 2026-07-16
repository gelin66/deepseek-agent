"""Small range utilities used by the production-path coding evaluation."""


def coalesce_ranges(values: list[int]) -> list[tuple[int, int]]:
    """Return sorted, inclusive ranges for the distinct integers in *values*."""
    numbers = sorted(set(values))
    if not numbers:
        return []

    ranges: list[tuple[int, int]] = []
    start = previous = numbers[0]
    for current in numbers[1:]:
        if current == previous + 1:
            previous = current
            continue
        ranges.append((start, previous))
        start = previous
        previous = current
    ranges.append((start, previous))
    return ranges
