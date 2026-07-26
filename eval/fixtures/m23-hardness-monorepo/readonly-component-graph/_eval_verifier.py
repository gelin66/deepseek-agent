#!/usr/bin/python3
from pathlib import Path
import json
import subprocess


root = Path(__file__).parent
program = r'''
import {
  schemaFloor,
  sharedCapabilities,
  startupOrder,
  runtimeMatrix,
} from "./deployment.ts";
console.log(JSON.stringify({schemaFloor, sharedCapabilities, startupOrder, runtimeMatrix}));
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
    "schemaFloor": 4,
    "sharedCapabilities": ["audit"],
    "startupOrder": ["policy", "ledger", "indexer", "worker"],
    "runtimeMatrix": {
        "indexer": "rust-1.97",
        "ledger": "python-3.9",
        "policy": "rust-1.97",
        "worker": "node-25",
    },
}
if actual != expected:
    raise SystemExit(1)
