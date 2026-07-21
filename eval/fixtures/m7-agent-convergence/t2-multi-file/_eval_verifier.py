from copy import deepcopy
from pathlib import Path
import importlib.util


ROOT = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("policy_merge", ROOT / "policy_merge.py")
module = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(module)

base = {
    "model": "flash",
    "limits": {"requests": 8, "tokens": 4096},
    "tools": ["read_file"],
}
override = {
    "limits": {"requests": 12},
    "tools": ["read_file", "run_verifiers"],
}
base_before = deepcopy(base)
override_before = deepcopy(override)
actual = module.merge_policy(base, override)
expected = {
    "model": "flash",
    "limits": {"requests": 12, "tokens": 4096},
    "tools": ["read_file", "run_verifiers"],
}
if actual != expected:
    raise SystemExit(f"unexpected merge: {actual!r}")
if base != base_before or override != override_before:
    raise SystemExit("merge_policy mutated an input")

tests = (ROOT / "test_policy_merge.py").read_text(encoding="utf-8")
for name in ("test_nested_limits_are_merged", "test_inputs_are_not_mutated"):
    if f"def {name}" not in tests:
        raise SystemExit(f"missing required regression test: {name}")
