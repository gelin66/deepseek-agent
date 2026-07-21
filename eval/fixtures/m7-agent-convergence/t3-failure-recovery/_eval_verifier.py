from pathlib import Path
import importlib.util


ROOT = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("retry_delay", ROOT / "retry_delay.py")
module = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(module)

expected = {0: 1, 1: 2, 2: 4, 4: 16, 5: 30, 20: 30}
for attempt, delay in expected.items():
    actual = module.retry_delay(attempt)
    if actual != delay:
        raise SystemExit(f"attempt {attempt}: {actual}, expected {delay}")
try:
    module.retry_delay(-1)
except ValueError:
    pass
else:
    raise SystemExit("negative attempt must raise ValueError")
