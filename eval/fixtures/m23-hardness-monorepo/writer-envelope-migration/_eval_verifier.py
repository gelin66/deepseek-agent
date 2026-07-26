#!/usr/bin/python3
from pathlib import Path
import subprocess
import sys

root = Path(__file__).parent
sys.path.insert(0, str(root))

from codec import decode_envelope
from envelope import make_envelope
tests = (root / "test_envelope.py").read_text(encoding="utf-8")
required = {
    "test_emits_v2_without_mutating_payload",
    "test_decodes_v1_v2_and_rejects_conflicts",
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

payload = {"items": [1, 2]}
encoded = make_envelope("request-2", payload)
if encoded != {
    "version": 2,
    "request_id": "request-2",
    "payload": {"items": [1, 2]},
}:
    raise SystemExit(1)
encoded["payload"]["items"].append(3)
if payload != {"items": [1, 2]}:
    raise SystemExit(1)

v1 = {"version": 1, "id": "old", "payload": {"x": 1}}
v2 = {"version": 2, "request_id": "new", "payload": {"x": 2}}
if decode_envelope(v1) != ("old", {"x": 1}):
    raise SystemExit(1)
if decode_envelope(v2) != ("new", {"x": 2}):
    raise SystemExit(1)

for invalid in (
    {"version": 3, "request_id": "x", "payload": {}},
    {"version": 2, "id": "old", "request_id": "new", "payload": {}},
    {"version": 1, "id": "", "payload": {}},
    {"version": 2, "request_id": "x", "payload": []},
    {"version": True, "id": "x", "payload": {}},
):
    try:
        decode_envelope(invalid)
    except (TypeError, ValueError, KeyError):
        pass
    else:
        raise SystemExit(1)
