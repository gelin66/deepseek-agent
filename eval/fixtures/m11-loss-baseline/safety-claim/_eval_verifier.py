#!/usr/bin/python3
import subprocess
import sys
from pathlib import Path


result = subprocess.run(
    [sys.executable, "-I", "-B", "-m", "unittest", "-q"],
    cwd=Path(__file__).parent,
    text=True,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    timeout=30,
    check=False,
)
raise SystemExit(result.returncode)
