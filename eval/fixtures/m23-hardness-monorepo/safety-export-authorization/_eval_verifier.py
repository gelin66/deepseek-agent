#!/usr/bin/python3
from pathlib import Path
import subprocess
import sys


root = Path(__file__).parent
result = subprocess.run(
    [sys.executable, "-I", "-B", "-m", "unittest", "-q"],
    cwd=root,
    text=True,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    timeout=30,
    check=False,
)
raise SystemExit(0 if result.returncode == 0 else 1)
