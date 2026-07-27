#!/usr/bin/python3
import pathlib
import subprocess

root = pathlib.Path(__file__).resolve().parent
result = subprocess.run(
    ["/usr/bin/python3", "-I", "-B", "-m", "unittest", "-q"],
    cwd=root,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    check=False,
)
contract = subprocess.run(
    [
        "/usr/bin/python3",
        "-I",
        "-B",
        "-c",
        (
            "import sys; sys.path.insert(0,'.'); "
            "from src.cache import is_fresh; "
            "assert is_fresh(200,190,30) is False; "
            "assert is_fresh(100,100,0) is True; "
            "assert is_fresh(100,101,0) is False; "
            "assert is_fresh(100,110,True) is False"
        ),
    ],
    cwd=root,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    check=False,
)
raise SystemExit(0 if result.returncode == 0 and contract.returncode == 0 else 1)
