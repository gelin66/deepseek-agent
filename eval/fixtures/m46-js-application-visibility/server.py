#!/usr/bin/python3
"""Serve one M46 JS-only fixture on the Host-assigned loopback origin."""

from __future__ import annotations

import http.server
import os
from pathlib import Path
import sys


ROOT = Path(__file__).resolve().parent
LEASE_PLACEHOLDER = "{{DSE_APPLICATION_PROBE_LEASE}}"


class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self) -> None:  # noqa: N802 - stdlib handler API
        if self.path == "/health":
            self._send(200, "text/plain; charset=utf-8", b"healthy")
            return
        if self.path != "/":
            self._send(404, "text/plain; charset=utf-8", b"not found")
            return
        source = (ROOT / os.environ["M46_FIXTURE_HTML"]).read_text(encoding="utf-8")
        body = source.replace(LEASE_PLACEHOLDER, sys.argv[-1]).encode("utf-8")
        self._send(200, "text/html; charset=utf-8", body)

    def _send(self, status: int, media_type: str, body: bytes) -> None:
        self.send_response(status)
        self.send_header("Content-Type", media_type)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, _format: str, *_args: object) -> None:
        return


server = http.server.ThreadingHTTPServer(
    (os.environ["HOST"], int(os.environ["PORT"])),
    Handler,
)
server.serve_forever()
