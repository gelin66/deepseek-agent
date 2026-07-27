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
            "import runpy,sys; sys.path.insert(0,'.'); "
            "runpy.run_path('_hidden_contract.py', run_name='__main__')"
        ),
    ],
    cwd=root,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    check=False,
)
raise SystemExit(0 if result.returncode == 0 and contract.returncode == 0 else 1)
