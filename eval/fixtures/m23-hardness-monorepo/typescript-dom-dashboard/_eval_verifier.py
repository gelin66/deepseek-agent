#!/usr/bin/python3
from pathlib import Path
import os
import socket
import subprocess
import sys
import time
import urllib.request


root = Path(__file__).parent
tests = (root / "test/state.test.js").read_text(encoding="utf-8")
required = {
    "keeps the empty-state contract",
    "preserves button semantics",
}
if not all(name in tests for name in required):
    raise SystemExit(1)

unit = subprocess.run(
    ["node", "--test", "test/state.test.js"],
    cwd=root,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    timeout=30,
    check=False,
)
if unit.returncode:
    sys.stderr.buffer.write(unit.stderr)
    raise SystemExit(1)

with socket.socket() as probe:
    probe.bind(("127.0.0.1", 0))
    port = probe.getsockname()[1]
process = subprocess.Popen(
    [sys.executable, "-I", "-B", "-m", "http.server", str(port), "--bind", "127.0.0.1"],
    cwd=root,
    stdin=subprocess.DEVNULL,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    start_new_session=True,
)
try:
    url = f"http://127.0.0.1:{port}/"
    for _ in range(100):
        try:
            urllib.request.urlopen(url, timeout=0.2).close()
            break
        except OSError:
            time.sleep(0.05)
    else:
        raise SystemExit(1)
    environment = dict(os.environ)
    environment["NODE_PATH"] = "/opt/homebrew/lib/node_modules"
    environment["M23_DOM_BASE_URL"] = url
    dom = subprocess.run(
        ["node", "test/dom_check.cjs"],
        cwd=root,
        env=environment,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=60,
        check=False,
    )
    if dom.returncode:
        sys.stderr.buffer.write(dom.stderr)
        raise SystemExit(1)
finally:
    process.terminate()
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=5)
