#!/usr/bin/python3
import json
import pathlib

root = pathlib.Path(__file__).resolve().parent
contract = json.loads((root / "generation.json").read_text(encoding="utf-8"))
source = root / contract["source"]
output = root / contract["output"]
ok = source.is_file() and output.is_file()
raise SystemExit(0 if ok else 1)
