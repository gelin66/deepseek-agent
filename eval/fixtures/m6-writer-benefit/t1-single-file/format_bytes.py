"""Small deterministic formatting utility used by the M6-B1 control task."""


def format_bytes(size: int) -> str:
    """Format a non-negative byte count with binary units."""
    if size < 0:
        raise ValueError("size must be non-negative")

    units = ("B", "KiB", "MiB", "GiB")
    value = float(size)
    for unit in units:
        if value < 1024 or unit == units[-1]:
            if unit == "B":
                return f"{int(value)} B"
            return f"{value:.1f} {unit}"
        value /= 1000

    raise AssertionError("unreachable")
