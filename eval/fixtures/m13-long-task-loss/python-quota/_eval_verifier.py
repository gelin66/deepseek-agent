#!/usr/bin/python3
import pathlib
import subprocess
import sys


root = pathlib.Path(sys.argv[1]).resolve()
result = subprocess.run(
    [sys.executable, "-I", "-B", "-m", "unittest", "-v"],
    cwd=root,
    stdin=subprocess.DEVNULL,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    check=False,
)
required = (
    "test_distributes_largest_remainders_stably",
    "test_respects_minimum_and_exact_total",
    "test_rejects_invalid_inputs",
)
visible = (root / "test_quota.py").read_text(encoding="utf-8")
hidden = (
    "import sys; sys.path.insert(0, '.'); from quota import allocate_quota; "
    "assert allocate_quota(10, [5, 3, 2]) == [5, 3, 2]; "
    "assert allocate_quota(11, [1, 1, 1]) == [4, 4, 3]; "
    "assert allocate_quota(10, [1, 2, 1], 1) == [3, 4, 3]"
)
hidden_result = subprocess.run(
    [sys.executable, "-I", "-B", "-c", hidden],
    cwd=root,
    stdin=subprocess.DEVNULL,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    check=False,
)
sys.exit(
    0
    if result.returncode == 0
    and hidden_result.returncode == 0
    and all(name in visible for name in required)
    else 1
)
