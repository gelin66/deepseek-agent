from pathlib import Path
import importlib.util


ROOT = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("impact", ROOT / "impact.py")
module = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(module)

expected = ["app", "runtime", "state", "tui"]
actual = module.affected_components("runtime")
if actual != expected:
    raise SystemExit(f"affected_components('runtime')={actual!r}, expected {expected!r}")

if module.affected_components("missing") != []:
    raise SystemExit("an unknown component must return []")
