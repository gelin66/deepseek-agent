#!/usr/bin/python3
from pathlib import Path
import subprocess
import sys


root = Path(__file__).parent
tests = (root / "test_worker.py").read_text(encoding="utf-8")
required = {
    "test_streams_canonical_unicode_responses",
    "test_rejects_invalid_request_without_output_for_that_line",
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

valid_input = (
    '{"id":"请求-2","value":4}\n'
    '{"id":"a","value":-3}\n'
).encode("utf-8")
valid = subprocess.run(
    [sys.executable, "-I", "-B", "worker.py"],
    cwd=root,
    input=valid_input,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    timeout=15,
    check=False,
)
expected = (
    '{"id":"请求-2","square":16}\n'
    '{"id":"a","square":9}\n'
).encode("utf-8")
if valid.returncode or valid.stdout != expected or valid.stderr:
    raise SystemExit(1)

invalid_cases = (
    b'{"id":"ok","value":2}\n{"id":"bad","value":true}\n',
    b'{"id":"ok","value":2}\n{"id":"","value":3}\n',
    b'{"id":"ok","value":2}\n{"id":"bad","value":3,"extra":1}\n',
    b'{"id":"ok","value":2}\nnot-json\n',
)
for payload in invalid_cases:
    result = subprocess.run(
        [sys.executable, "-I", "-B", "worker.py"],
        cwd=root,
        input=payload,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=15,
        check=False,
    )
    if (
        result.returncode != 2
        or result.stdout != b'{"id":"ok","square":4}\n'
        or result.stderr != b"invalid request at line 2\n"
    ):
        raise SystemExit(1)
