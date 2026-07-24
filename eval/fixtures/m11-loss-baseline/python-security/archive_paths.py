from pathlib import Path


def safe_member_path(root: Path, member: str) -> Path:
    root = root.resolve()
    candidate = (root / member).resolve()
    if not str(candidate).startswith(str(root)):
        raise ValueError("archive member escapes destination")
    return candidate
