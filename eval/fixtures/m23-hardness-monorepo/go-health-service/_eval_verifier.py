#!/usr/bin/python3
from pathlib import Path
import json
import os
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request


root = Path(__file__).parent
tests = (root / "internal/httpapi/handler_test.go").read_text(encoding="utf-8")
required = {
    "TestHealthReturnsCanonicalJSON",
    "TestHealthRejectsUnsupportedMethods",
}
if not all(name in tests for name in required):
    raise SystemExit(1)


with tempfile.TemporaryDirectory(prefix="dse-m23-go-cache-") as cache:
    environment = dict(os.environ)
    environment["GOCACHE"] = cache
    checked = subprocess.run(
        ["go", "test", "./..."],
        cwd=root,
        env=environment,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=90,
        check=False,
    )
    if checked.returncode:
        sys.stderr.buffer.write(checked.stderr)
        raise SystemExit(1)

    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        port = probe.getsockname()[1]
    environment["LISTEN_ADDR"] = f"127.0.0.1:{port}"
    environment["SERVICE_REGION"] = "fixture"
    process = subprocess.Popen(
        ["go", "run", "./cmd/server"],
        cwd=root,
        env=environment,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        start_new_session=True,
    )
    try:
        url = f"http://127.0.0.1:{port}/healthz?deep=1"
        response = None
        for _ in range(100):
            try:
                response = urllib.request.urlopen(url, timeout=0.2)
                break
            except (OSError, urllib.error.URLError):
                time.sleep(0.05)
        if response is None:
            raise SystemExit(1)
        with response:
            payload = response.read()
            if (
                response.status != 200
                or response.headers.get("Content-Type") != "application/json"
                or json.loads(payload)
                != {
                    "deep": True,
                    "region": "fixture",
                    "status": "ok",
                    "version": "2026.07",
                }
            ):
                raise SystemExit(1)
        request = urllib.request.Request(url, method="POST")
        try:
            urllib.request.urlopen(request, timeout=1)
        except urllib.error.HTTPError as error:
            if error.code != 405 or error.headers.get("Allow") != "GET, HEAD":
                raise SystemExit(1)
        else:
            raise SystemExit(1)
    finally:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)
