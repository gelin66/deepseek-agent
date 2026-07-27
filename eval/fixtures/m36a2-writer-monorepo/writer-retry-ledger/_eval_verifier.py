#!/usr/bin/python3
from pathlib import Path
import subprocess
import sys


root = Path(sys.argv[1]).resolve()
sys.path.insert(0, str(root))

from codec import encode_summary
from ledger import summarize_attempts

checked = subprocess.run(
    [sys.executable, "-I", "-B", "-m", "unittest", "-q"],
    cwd=root,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    timeout=30,
    check=False,
)
if checked.returncode:
    sys.stderr.buffer.write(checked.stderr)
    raise SystemExit(1)

records = [
    {
        "attempt_id": "a1",
        "logical_id": "r1",
        "status": "completed",
        "input_tokens": 11,
        "output_tokens": 5,
        "cost_nanousd": 23,
    },
    {
        "attempt_id": "a2",
        "logical_id": "r1",
        "status": "started",
        "input_tokens": None,
        "output_tokens": None,
        "cost_nanousd": None,
    },
    {
        "attempt_id": "a3",
        "logical_id": "r2",
        "status": "completed",
        "input_tokens": 7,
        "output_tokens": 2,
        "cost_nanousd": 13,
    },
]
expected = {
    "physical_started": 3,
    "physical_completed": 2,
    "physical_in_flight": 1,
    "logical_requests": 2,
    "usage_complete": False,
    "input_tokens": 18,
    "output_tokens": 7,
    "cost_nanousd": 36,
}
if summarize_attempts(records) != expected:
    raise SystemExit(1)
encoded = encode_summary(records)
if encoded != {"version": 2, "accounting": expected}:
    raise SystemExit(1)
encoded["accounting"]["input_tokens"] = 999
if summarize_attempts(records) != expected:
    raise SystemExit(1)

invalid = [
    "not-a-list",
    [{**records[0], "attempt_id": ""}],
    [records[0], dict(records[0])],
    [{**records[0], "status": "unknown"}],
    [{**records[0], "input_tokens": True}],
    [{**records[0], "cost_nanousd": -1}],
    [{**records[1], "input_tokens": 0}],
    [{**records[0], "extra": 1}],
]
for value in invalid:
    try:
        summarize_attempts(value)
    except (TypeError, ValueError):
        pass
    else:
        raise SystemExit(1)
