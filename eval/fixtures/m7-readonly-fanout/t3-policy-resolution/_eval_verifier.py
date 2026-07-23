from pathlib import Path
import importlib.util


ROOT = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("policy", ROOT / "policy.py")
module = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(module)

expected = {
    "provider": "deepseek",
    "runtime": {
        "store": "sqlite",
        "limits": {
            "requests": 12,
            "children": 2,
        },
    },
    "display": {
        "language": "zh-CN",
        "compact": True,
        "theme": "underwater",
    },
}
actual = module.resolve_policy()
if actual != expected:
    raise SystemExit(f"resolve_policy()={actual!r}, expected {expected!r}")
