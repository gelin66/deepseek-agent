from pathlib import Path
import importlib.util


ROOT = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("runtime_banner", ROOT / "runtime_banner.py")
module = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(module)

expected = "DeepSeek Agent | provider=DeepSeek | requests=12 | children=0"
actual = module.runtime_banner()
if actual != expected:
    raise SystemExit(f"runtime_banner()={actual!r}, expected {expected!r}")
