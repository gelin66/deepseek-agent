#!/usr/bin/python3
from pathlib import Path
import json
import subprocess


root = Path(__file__).parent
program = r'''
import {
  protocolFloor,
  sharedFeatures,
  startupOrder,
  runtimeMatrix,
} from "./compatibility.ts";
console.log(JSON.stringify({protocolFloor, sharedFeatures, startupOrder, runtimeMatrix}));
'''
result = subprocess.run(
    ["node", "--experimental-strip-types", "--input-type=module", "--eval", program],
    cwd=root,
    text=True,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    timeout=30,
    check=False,
)
if result.returncode:
    raise SystemExit(1)
actual = json.loads(result.stdout)
expected = {
    "protocolFloor": 3,
    "sharedFeatures": ["tracing"],
    "startupOrder": ["auth", "billing", "gateway", "worker"],
    "runtimeMatrix": {
        "auth": "rust-1.97",
        "billing": "python-3.9",
        "gateway": "node-25",
        "worker": "python-3.9",
    },
}
if actual != expected:
    raise SystemExit(1)
