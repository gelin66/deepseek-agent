#!/usr/bin/python3
from pathlib import Path
import json
import subprocess


root = Path(__file__).parent
program = """
import {
  minimumNode,
  sharedScripts,
  sharedFeatures,
  packageOrder,
} from "./compatibility.ts";
console.log(JSON.stringify({minimumNode, sharedScripts, sharedFeatures, packageOrder}));
"""
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
    "minimumNode": 22,
    "sharedScripts": ["build", "test"],
    "sharedFeatures": ["oauth"],
    "packageOrder": ["auth", "api", "web"],
}
if actual != expected:
    raise SystemExit(1)
