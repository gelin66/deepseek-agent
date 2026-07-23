from pathlib import Path
import importlib.util


ROOT = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("compatibility", ROOT / "compatibility.py")
module = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(module)

expected = {
    "minimum": 5,
    "shared_features": ["evidence_receipts"],
    "components": ["parser", "runtime", "state"],
}
actual = module.compatibility()
if actual != expected:
    raise SystemExit(f"compatibility()={actual!r}, expected {expected!r}")
