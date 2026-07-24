#!/usr/bin/python3
from pathlib import Path
import subprocess
import sys
import tempfile

from archive_paths import safe_member_path


root = Path(__file__).parent
tests = (root / "test_archive_paths.py").read_text(encoding="utf-8")
required = {
    "test_rejects_sibling_prefix_escape",
    "test_rejects_absolute_and_backslash_members",
}
if not all(name in tests for name in required):
    raise SystemExit(1)

checked = subprocess.run(
    [sys.executable, "-I", "-B", "-m", "unittest", "-q"],
    cwd=root,
    text=True,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    timeout=30,
    check=False,
)
if checked.returncode:
    sys.stderr.write(checked.stderr)
    raise SystemExit(1)

with tempfile.TemporaryDirectory() as raw:
    destination = Path(raw) / "out"
    destination.mkdir()
    accepted = {
        "assets/app.js": destination / "assets/app.js",
        "README.md": destination / "README.md",
    }
    for member, expected in accepted.items():
        if safe_member_path(destination, member) != expected:
            raise SystemExit(1)
    for member in (
        "../out-evil/payload",
        "/etc/passwd",
        r"..\escape.txt",
        "",
        ".",
        "assets/../../escape",
    ):
        try:
            safe_member_path(destination, member)
        except ValueError:
            pass
        else:
            raise SystemExit(1)
