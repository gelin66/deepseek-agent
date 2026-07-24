from pathlib import Path


def destination(root: Path, member: str) -> Path:
    return root / member
